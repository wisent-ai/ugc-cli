use super::*;

impl<'a> StandaloneService<'a> {
    pub fn resolve_portal(&self, token: &str) -> Result<PortalAccess> {
        let hash = hash_token(token);
        let mut access: PortalAccess = self
            .store
            .find_external("portal_access", &hash)?
            .context("invalid portal token")?;
        if access.status != "active" {
            bail!("portal access is not active");
        }
        if let Some(expires_at) = &access.expires_at {
            let expiry = DateTime::parse_from_rfc3339(expires_at)?;
            if expiry < Utc::now() {
                bail!("portal access has expired");
            }
        }
        access.last_used_at = Some(Store::now());
        self.store.put(
            "portal_access",
            &access.id,
            Some(&access.creator_id),
            None,
            &access.status,
            Some(&access.token_hash),
            &access,
            &access.created_at,
        )?;
        Ok(access)
    }
    pub fn revoke_portal(&self, access_id: &str) -> Result<PortalAccess> {
        let mut access: PortalAccess = self.store.get("portal_access", access_id)?;
        access.status = "revoked".into();
        self.store.put(
            "portal_access",
            &access.id,
            Some(&access.creator_id),
            None,
            &access.status,
            Some(&access.token_hash),
            &access,
            &access.created_at,
        )?;
        self.audit("portal_access", access_id, "revoked", json!({}))?;
        Ok(access)
    }
    pub fn fund_escrow(
        &self,
        assignment_id: &str,
        amount_minor: i64,
        currency: String,
        idempotency_key: String,
    ) -> Result<LedgerTransfer> {
        self.transfer(
            "external:funding".into(),
            format!("escrow:{assignment_id}"),
            amount_minor,
            currency,
            "escrow_funding".into(),
            Some(assignment_id.into()),
            None,
            None,
            idempotency_key,
            None,
        )
    }
    pub fn release_payment(&self, payment_id: &str, idempotency_key: String) -> Result<Value> {
        let payment: Payment = self.store.get("payment", payment_id)?;
        if payment.owner != "echo" {
            bail!("only Echo-owned payments use the standalone ledger");
        }
        if payment.status == "paid" {
            return Ok(json!({"payment": payment, "already_paid": true}));
        }
        if payment.status == "processing" {
            let transfers: Vec<LedgerTransfer> =
                self.store
                    .list("ledger_transfer", Some(&payment.assignment_id), None)?;
            let transfer = transfers.into_iter().find(|transfer| {
                transfer.payment_id.as_deref() == Some(payment_id)
                    && transfer.kind == "creator_release"
            });
            return Ok(json!({"payment": payment, "already_released": true, "transfer": transfer}));
        }
        if payment.status != "pending" {
            bail!("payment must be pending before release");
        }
        let assignment: Assignment = self.store.get("assignment", &payment.assignment_id)?;
        let submission_id = payment
            .submission_id
            .as_deref()
            .context("payment has no approved submission")?;
        let submission: Submission = self.store.get("submission", submission_id)?;
        if submission.status != "approved" {
            bail!("submission must be approved before release");
        }
        let assets: Vec<Asset> = self.store.list("asset", Some(&submission.id), None)?;
        let rights: Vec<UsageRights> =
            self.store
                .list("usage_rights", Some(&assignment.id), None)?;
        let applicable_rights: Vec<&UsageRights> = rights
            .iter()
            .filter(|rights| {
                rights.asset_id.is_none()
                    || assets
                        .iter()
                        .any(|asset| rights.asset_id.as_deref() == Some(&asset.id))
            })
            .collect();
        if applicable_rights.is_empty() {
            bail!("usage rights for the approved submission must be recorded before release");
        }
        if applicable_rights
            .iter()
            .any(|rights| !rights.model_release || !rights.music_cleared)
        {
            bail!("applicable rights must confirm model release and music clearance");
        }
        let escrow = format!("escrow:{}", assignment.id);
        let balance = self.balance(&escrow, &payment.currency)?.balance_minor;
        if balance < payment.amount_minor {
            bail!(
                "escrow balance {balance} is below payment amount {}",
                payment.amount_minor
            );
        }
        let transfer = self.transfer(
            escrow,
            format!("creator:{}", assignment.creator_id),
            payment.amount_minor,
            payment.currency.clone(),
            "creator_release".into(),
            Some(assignment.id.clone()),
            Some(payment.id.clone()),
            None,
            idempotency_key,
            None,
        )?;
        let payment = self
            .core()
            .payment_status(payment_id, "processing", None, None)?;
        Ok(json!({"payment": payment, "transfer": transfer}))
    }
    pub fn settle_offline(
        &self,
        payment_id: &str,
        reference: String,
        idempotency_key: String,
    ) -> Result<Value> {
        if reference.trim().is_empty() {
            bail!("offline settlement reference is required");
        }
        let payment: Payment = self.store.get("payment", payment_id)?;
        if payment.status == "paid" {
            return Ok(json!({"payment": payment, "already_paid": true}));
        }
        if payment.status != "processing" {
            bail!("payment must be released to creator balance before settlement");
        }
        let assignment: Assignment = self.store.get("assignment", &payment.assignment_id)?;
        let account = format!("creator:{}", assignment.creator_id);
        let balance = self.balance(&account, &payment.currency)?.balance_minor;
        if balance < payment.amount_minor {
            bail!("creator ledger balance is below payment amount");
        }
        let transfer = self.transfer(
            account,
            "external:offline_payout".into(),
            payment.amount_minor,
            payment.currency.clone(),
            "offline_settlement".into(),
            Some(assignment.id.clone()),
            Some(payment.id.clone()),
            Some(reference.clone()),
            idempotency_key,
            None,
        )?;
        let payment = self
            .core()
            .payment_status(payment_id, "paid", Some(reference), None)?;
        Ok(json!({"payment": payment, "transfer": transfer}))
    }
    pub fn reverse_transfer(
        &self,
        transfer_id: &str,
        reason: String,
        idempotency_key: String,
    ) -> Result<LedgerTransfer> {
        if reason.trim().is_empty() {
            bail!("reversal reason is required");
        }
        let original: LedgerTransfer = self.store.get("ledger_transfer", transfer_id)?;
        if original.status != "posted" {
            bail!("only posted transfers can be reversed");
        }
        let existing: Vec<LedgerTransfer> = self.store.list("ledger_transfer", None, None)?;
        if existing
            .iter()
            .any(|transfer| transfer.reversal_of.as_deref() == Some(transfer_id))
        {
            bail!("transfer already has a reversal");
        }
        self.transfer(
            original.to_account,
            original.from_account,
            original.amount_minor,
            original.currency,
            "reversal".into(),
            original.assignment_id,
            original.payment_id,
            Some(reason),
            idempotency_key,
            Some(transfer_id.into()),
        )
    }
    pub fn balance(&self, account: &str, currency: &str) -> Result<LedgerBalance> {
        let transfers: Vec<LedgerTransfer> =
            self.store.list("ledger_transfer", None, Some("posted"))?;
        let mut balance: i64 = 0;
        for transfer in transfers
            .iter()
            .filter(|transfer| transfer.currency.eq_ignore_ascii_case(currency))
        {
            if transfer.to_account == account {
                balance += transfer.amount_minor;
            }
            if transfer.from_account == account {
                balance -= transfer.amount_minor;
            }
        }
        Ok(LedgerBalance {
            account: account.into(),
            currency: currency.into(),
            balance_minor: balance,
        })
    }
    pub fn ledger(&self, assignment_id: Option<&str>) -> Result<Vec<LedgerTransfer>> {
        let transfers: Vec<LedgerTransfer> =
            self.store.list("ledger_transfer", assignment_id, None)?;
        Ok(transfers)
    }
}
