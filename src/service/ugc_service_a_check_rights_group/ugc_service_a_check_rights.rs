use super::*;

impl<'a> UgcService<'a> {
    pub fn check_rights(
        &self,
        assignment_id: &str,
        asset_id: Option<&str>,
        channel: &str,
        territory: Option<&str>,
        paid: bool,
        at: Option<&str>,
    ) -> Result<Value> {
        let _: Assignment = self.store.get("assignment", assignment_id)?;
        if let Some(asset_id) = asset_id {
            let asset: Asset = self.store.get("asset", asset_id)?;
            let Some(submission_id) = asset.submission_id else {
                bail!("rights check asset must belong to a submission");
            };
            let submission: Submission = self.store.get("submission", &submission_id)?;
            if submission.assignment_id != assignment_id {
                bail!("rights check asset does not belong to assignment");
            }
        }
        let rights: Vec<UsageRights> =
            self.store
                .list("usage_rights", Some(assignment_id), Some("active"))?;
        let instant = match at {
            Some(value) => DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc),
            None => Utc::now(),
        };
        let mut reasons = Vec::new();
        let mut allowed = false;
        for item in rights {
            if item.asset_id.is_some() && item.asset_id.as_deref() != asset_id {
                reasons.push("asset not licensed".to_string());
                continue;
            }
            let starts_at = DateTime::parse_from_rfc3339(&item.starts_at)?.with_timezone(&Utc);
            if starts_at > instant {
                reasons.push("license has not started".to_string());
                continue;
            }
            if let Some(expires_at) = &item.expires_at {
                let expires_at = DateTime::parse_from_rfc3339(expires_at)?.with_timezone(&Utc);
                if expires_at <= instant {
                    reasons.push("license expired".to_string());
                    continue;
                }
            }
            if !item.channels.is_empty()
                && !item
                    .channels
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(channel))
            {
                reasons.push(format!("channel {channel} not licensed"));
                continue;
            }
            if !item.territories.is_empty() {
                let Some(territory) = territory else {
                    reasons.push("territory is required by the license".into());
                    continue;
                };
                if !item
                    .territories
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(territory))
                {
                    reasons.push(format!("territory {territory} not licensed"));
                    continue;
                }
            }
            if paid && !item.paid_ads_allowed {
                reasons.push("paid ads not licensed".into());
                continue;
            }
            if !paid && !item.organic_allowed {
                reasons.push("organic usage not licensed".into());
                continue;
            }
            if !item.model_release {
                reasons.push("model release missing".into());
                continue;
            }
            if !item.music_cleared {
                reasons.push("music clearance missing".into());
                continue;
            }
            allowed = true;
            break;
        }
        if !allowed && reasons.is_empty() {
            reasons.push("no active usage rights".into());
        }
        Ok(json!({
            "allowed": allowed,
            "assignment_id": assignment_id,
            "asset_id": asset_id,
            "channel": channel,
            "territory": territory,
            "paid": paid,
            "reasons": reasons,
        }))
    }
    pub fn create_payment(
        &self,
        assignment_id: String,
        submission_id: Option<String>,
        amount_minor: i64,
        currency: String,
        external_payment_id: Option<String>,
    ) -> Result<Payment> {
        let assignment: Assignment = self.store.get("assignment", &assignment_id)?;
        if assignment.payment_owner == "none" {
            bail!("assignment payment owner is none");
        }
        required("currency", &currency)?;
        if amount_minor <= i64::default() {
            bail!("payment amount must be positive");
        }
        if !currency.eq_ignore_ascii_case(&assignment.currency) {
            bail!("payment currency must match assignment currency");
        }
        let currency = assignment.currency.clone();
        if assignment
            .compensation_minor
            .is_some_and(|compensation| compensation != amount_minor)
        {
            bail!("payment amount must match assignment compensation");
        }
        if let Some(submission) = &submission_id {
            let submission: Submission = self.store.get("submission", submission)?;
            if submission.assignment_id != assignment_id || submission.status != "approved" {
                bail!("payment submission must be approved and belong to assignment");
            }
        }
        let key = format!(
            "{}:{}:{}:{}",
            assignment_id,
            submission_id.as_deref().unwrap_or("assignment"),
            amount_minor,
            currency
        );
        if let Some(existing) = self.store.find_external::<Payment>("payment", &key)? {
            return Ok(existing);
        }
        let now = Store::now();
        let payment = Payment {
            id: Store::id(),
            assignment_id: assignment_id.clone(),
            submission_id,
            owner: assignment.payment_owner.clone(),
            amount_minor,
            currency,
            status: if assignment.payment_owner == "provider" {
                "external_pending".into()
            } else {
                "pending".into()
            },
            external_payment_id,
            idempotency_key: key.clone(),
            error: None,
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.put(
            "payment",
            &payment.id,
            Some(&assignment_id),
            None,
            &payment.status,
            Some(&key),
            &payment,
            &payment.created_at,
        )?;
        self.audit(
            "payment",
            &payment.id,
            "created",
            json!({"owner": payment.owner, "amount_minor": amount_minor}),
        )?;
        Ok(payment)
    }
    pub fn payment_status(
        &self,
        id: &str,
        status: &str,
        external_payment_id: Option<String>,
        error: Option<String>,
    ) -> Result<Payment> {
        let mut payment: Payment = self.store.get("payment", id)?;
        ensure_transition("payment", &payment.status, status)?;
        payment.status = status.into();
        payment.external_payment_id = external_payment_id.or(payment.external_payment_id);
        payment.error = error;
        payment.updated_at = Store::now();
        self.store.put(
            "payment",
            &payment.id,
            Some(&payment.assignment_id),
            None,
            &payment.status,
            Some(&payment.idempotency_key),
            &payment,
            &payment.created_at,
        )?;
        self.audit(
            "payment",
            id,
            "status_changed",
            json!({"status": status, "external_payment_id": payment.external_payment_id}),
        )?;
        Ok(payment)
    }
    pub fn send_message(
        &self,
        assignment_id: String,
        direction: String,
        channel: String,
        body: String,
    ) -> Result<Message> {
        let assignment: Assignment = self.store.get("assignment", &assignment_id)?;
        required("body", &body)?;
        let message = Message {
            id: Store::id(),
            assignment_id: assignment_id.clone(),
            direction,
            channel,
            body,
            external_message_id: None,
            created_at: Store::now(),
        };
        self.store.put(
            "message",
            &message.id,
            Some(&assignment_id),
            None,
            "created",
            None,
            &message,
            &message.created_at,
        )?;
        if message.direction == "outbound" {
            if let Some(connection) = assignment.connection_id {
                self.store.enqueue(
                    "send_message",
                    "message",
                    &message.id,
                    Some(&connection),
                    &serde_json::to_value(&message)?,
                )?;
            }
        }
        self.audit(
            "assignment",
            &assignment_id,
            "message_created",
            json!({"message_id": message.id, "direction": message.direction}),
        )?;
        Ok(message)
    }
}
