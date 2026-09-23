use super::*;

impl<'a> UgcService<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn create_assignment(
        &self,
        campaign_id: String,
        brief_id: String,
        creator_id: String,
        connection_id: Option<String>,
        compensation_minor: Option<i64>,
        currency: String,
        payment_owner: String,
        deadline: Option<String>,
        shipping_required: bool,
        revision_limit: Option<i64>,
        external_assignment_id: Option<String>,
    ) -> Result<Assignment> {
        let campaign: Campaign = self.store.get("campaign", &campaign_id)?;
        let brief: Brief = self.store.get("brief", &brief_id)?;
        if brief.campaign_id != campaign_id {
            bail!("brief does not belong to campaign");
        }
        if brief.status != "approved" {
            bail!("brief must be approved before assignment");
        }
        let _: Creator = self.store.get("creator", &creator_id)?;
        if let Some(connection) = &connection_id {
            let _: Connection = self.store.get("connection", connection)?;
        }
        if !matches!(payment_owner.as_str(), "provider" | "echo" | "none") {
            bail!("payment_owner must be provider, echo, or none");
        }
        required("currency", &currency)?;
        if !currency.eq_ignore_ascii_case(&campaign.currency) {
            bail!("assignment currency must match campaign currency");
        }
        let currency = campaign.currency.clone();
        if compensation_minor.is_some_and(|amount| amount < i64::default()) {
            bail!("assignment compensation cannot be negative");
        }
        if revision_limit.is_some_and(|limit| limit < i64::default()) {
            bail!("assignment revision limit cannot be negative");
        }
        if external_assignment_id
            .as_deref()
            .is_some_and(|external| external.trim().is_empty())
        {
            bail!("external assignment ID cannot be empty");
        }
        let external_assignment_id =
            external_assignment_id.map(|external| external.trim().to_string());
        if let Some(external) = external_assignment_id.as_deref() {
            if let Some(existing) = self
                .store
                .find_external::<Assignment>("assignment", external)?
            {
                let same_assignment = existing.campaign_id == campaign_id
                    && existing.brief_id == brief_id
                    && existing.creator_id == creator_id
                    && existing.connection_id == connection_id
                    && existing.compensation_minor == compensation_minor
                    && existing.currency.eq_ignore_ascii_case(&currency)
                    && existing.payment_owner == payment_owner
                    && existing.deadline == deadline
                    && existing.shipping_required == shipping_required
                    && existing.revision_limit == revision_limit;
                if same_assignment {
                    return Ok(existing);
                }
                bail!("external assignment ID was already used: {external}");
            }
        }
        let campaign_deadline = campaign
            .deadline
            .as_deref()
            .map(DateTime::parse_from_rfc3339)
            .transpose()
            .context("invalid campaign deadline")?;
        if campaign_deadline
            .as_ref()
            .is_some_and(|deadline| *deadline <= Utc::now())
        {
            bail!("campaign deadline has passed");
        }
        if let Some(deadline) = deadline.as_deref() {
            let assignment_deadline =
                DateTime::parse_from_rfc3339(deadline).context("invalid assignment deadline")?;
            if assignment_deadline <= Utc::now() {
                bail!("assignment deadline must be in the future");
            }
            if campaign_deadline
                .as_ref()
                .is_some_and(|campaign_deadline| assignment_deadline > *campaign_deadline)
            {
                bail!("assignment deadline cannot exceed campaign deadline");
            }
        }
        if let (Some(budget), Some(compensation)) = (campaign.budget_minor, compensation_minor) {
            let existing: Vec<Assignment> =
                self.store.list("assignment", Some(&campaign_id), None)?;
            let mut committed = i64::default();
            for assignment in existing
                .into_iter()
                .filter(|assignment| !matches!(assignment.status.as_str(), "cancelled" | "failed"))
            {
                if let Some(amount) = assignment.compensation_minor {
                    let Some(total) = committed.checked_add(amount) else {
                        bail!("campaign committed compensation overflow");
                    };
                    committed = total;
                }
            }
            let Some(total) = committed.checked_add(compensation) else {
                bail!("campaign committed compensation overflow");
            };
            if total > budget {
                bail!("assignment compensation would exceed campaign budget");
            }
        }
        let now = Store::now();
        let assignment = Assignment {
            id: Store::id(),
            campaign_id: campaign_id.clone(),
            brief_id,
            creator_id,
            connection_id,
            external_assignment_id: external_assignment_id.clone(),
            status: "invited".into(),
            compensation_minor,
            currency,
            payment_owner,
            deadline,
            shipping_required,
            revision_limit,
            accepted_at: None,
            completed_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.put(
            "assignment",
            &assignment.id,
            Some(&campaign_id),
            Some(&assignment.creator_id),
            &assignment.status,
            external_assignment_id.as_deref(),
            &assignment,
            &assignment.created_at,
        )?;
        self.audit(
            "assignment",
            &assignment.id,
            "created",
            json!({"campaign_id": campaign_id}),
        )?;
        Ok(assignment)
    }
    pub fn assignment_status(&self, id: &str, status: &str) -> Result<Assignment> {
        let mut assignment: Assignment = self.store.get("assignment", id)?;
        ensure_transition("assignment", &assignment.status, status)?;
        assignment.status = status.into();
        assignment.updated_at = Store::now();
        if status == "accepted" && assignment.accepted_at.is_none() {
            assignment.accepted_at = Some(Store::now());
        }
        if status == "completed" {
            assignment.completed_at = Some(Store::now());
        }
        self.store.put(
            "assignment",
            &assignment.id,
            Some(&assignment.campaign_id),
            Some(&assignment.creator_id),
            &assignment.status,
            assignment.external_assignment_id.as_deref(),
            &assignment,
            &assignment.created_at,
        )?;
        self.audit(
            "assignment",
            id,
            "status_changed",
            json!({"status": status}),
        )?;
        Ok(assignment)
    }
    pub fn update_shipment(
        &self,
        assignment_id: String,
        status: String,
        carrier: Option<String>,
        tracking_number: Option<String>,
        product_variant: Option<String>,
        shipping_address: Option<ShippingAddress>,
    ) -> Result<Shipment> {
        let assignment: Assignment = self.store.get("assignment", &assignment_id)?;
        if !assignment.shipping_required {
            bail!("assignment does not require shipping");
        }
        let existing: Vec<Shipment> = self.store.list("shipment", Some(&assignment_id), None)?;
        let now = Store::now();
        let mut shipment = existing.into_iter().next().unwrap_or(Shipment {
            id: Store::id(),
            assignment_id: assignment_id.clone(),
            status: "awaiting_address".into(),
            carrier: None,
            tracking_number: None,
            product_variant: None,
            shipping_address: None,
            shipped_at: None,
            delivered_at: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        });
        ensure_transition("shipment", &shipment.status, &status)?;
        shipment.status = status.clone();
        shipment.carrier = carrier.or(shipment.carrier);
        shipment.tracking_number = tracking_number.or(shipment.tracking_number);
        shipment.product_variant = product_variant.or(shipment.product_variant);
        shipment.shipping_address = shipping_address.or(shipment.shipping_address);
        if let Some(address) = &shipment.shipping_address {
            required("shipping recipient", &address.recipient_name)?;
            required("shipping address line", &address.line1)?;
            required("shipping city", &address.city)?;
            required("shipping postal code", &address.postal_code)?;
            required("shipping country", &address.country)?;
        }
        if status == "ready_to_ship" && shipment.shipping_address.is_none() {
            bail!("shipping address is required before shipment is ready");
        }
        if status == "shipped"
            && (shipment
                .carrier
                .as_deref()
                .is_none_or(|carrier| carrier.trim().is_empty())
                || shipment
                    .tracking_number
                    .as_deref()
                    .is_none_or(|tracking| tracking.trim().is_empty()))
        {
            bail!("carrier and tracking number are required before shipment is shipped");
        }
        shipment.updated_at = now;
        if status == "shipped" {
            shipment.shipped_at = Some(Store::now());
        }
        if status == "delivered" {
            shipment.delivered_at = Some(Store::now());
        }
        self.store.put(
            "shipment",
            &shipment.id,
            Some(&assignment_id),
            None,
            &shipment.status,
            shipment.tracking_number.as_deref(),
            &shipment,
            &shipment.created_at,
        )?;
        self.audit(
            "assignment",
            &assignment_id,
            "shipment_updated",
            json!({"status": status}),
        )?;
        Ok(shipment)
    }
}
