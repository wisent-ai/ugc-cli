use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::{
    db::Store,
    model::{
        Assignment, Brief, Campaign, Connection, Creator, CreatorIdentity, Message, Payment,
        ProviderCapabilities, Publication, Shipment, Submission, UsageRights,
    },
};

pub struct UgcService<'a> {
    pub store: &'a Store,
    pub actor: &'a str,
}

impl<'a> UgcService<'a> {
    pub fn add_connection(
        &self,
        name: String,
        provider: String,
        base_url: Option<String>,
        token_env: Option<String>,
        webhook_secret_env: Option<String>,
        external_account_id: Option<String>,
    ) -> Result<Connection> {
        if provider != "manual" && provider != "http" {
            bail!("unsupported provider '{provider}'; supported providers: manual, http");
        }
        if provider == "http" && base_url.is_none() {
            bail!("http provider requires --base-url");
        }
        let now = Store::now();
        let connection = Connection {
            id: Store::id(),
            name,
            provider: provider.clone(),
            status: "active".into(),
            base_url,
            token_env,
            webhook_secret_env,
            external_account_id,
            capabilities: if provider == "manual" {
                ProviderCapabilities::manual()
            } else {
                ProviderCapabilities::http()
            },
            sync_cursor: None,
            last_sync_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.put(
            "connection",
            &connection.id,
            None,
            None,
            &connection.status,
            None,
            &connection,
            &connection.created_at,
        )?;
        self.audit(
            "connection",
            &connection.id,
            "created",
            json!({"provider": connection.provider}),
        )?;
        Ok(connection)
    }

    pub fn create_campaign(
        &self,
        name: String,
        brand: String,
        product: String,
        objective: String,
        markets: Vec<String>,
        languages: Vec<String>,
        channels: Vec<String>,
        budget_minor: Option<i64>,
        currency: String,
        deadline: Option<String>,
    ) -> Result<Campaign> {
        required("name", &name)?;
        required("brand", &brand)?;
        required("product", &product)?;
        let now = Store::now();
        let campaign = Campaign {
            id: Store::id(),
            name,
            brand,
            product,
            objective,
            markets,
            languages,
            channels,
            budget_minor,
            currency,
            deadline,
            status: "draft".into(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.put(
            "campaign",
            &campaign.id,
            None,
            None,
            &campaign.status,
            None,
            &campaign,
            &campaign.created_at,
        )?;
        self.audit("campaign", &campaign.id, "created", json!({}))?;
        Ok(campaign)
    }

    pub fn campaign_status(&self, id: &str, status: &str) -> Result<Campaign> {
        let mut campaign: Campaign = self.store.get("campaign", id)?;
        ensure_transition("campaign", &campaign.status, status)?;
        campaign.status = status.into();
        campaign.updated_at = Store::now();
        self.store.put(
            "campaign",
            &campaign.id,
            None,
            None,
            &campaign.status,
            None,
            &campaign,
            &campaign.created_at,
        )?;
        self.audit("campaign", id, "status_changed", json!({"status": status}))?;
        Ok(campaign)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_brief(
        &self,
        campaign_id: String,
        service_type: String,
        creative_angle: String,
        requirements: Vec<String>,
        forbidden_claims: Vec<String>,
        required_shots: Vec<String>,
        talking_points: Vec<String>,
        cta: Option<String>,
        duration_min_ms: Option<i64>,
        duration_max_ms: Option<i64>,
        aspect_ratios: Vec<String>,
        raw_footage_required: bool,
        revision_limit: Option<i64>,
        rights_requirements: Value,
    ) -> Result<Brief> {
        let _: Campaign = self.store.get("campaign", &campaign_id)?;
        let existing: Vec<Brief> = self.store.list("brief", Some(&campaign_id), None)?;
        let version = existing
            .iter()
            .map(|brief| brief.version)
            .max()
            .unwrap_or("".len() as i64)
            + "v".len() as i64;
        let now = Store::now();
        let brief = Brief {
            id: Store::id(),
            campaign_id: campaign_id.clone(),
            version,
            service_type,
            creative_angle,
            requirements,
            forbidden_claims,
            required_shots,
            talking_points,
            cta,
            duration_min_ms,
            duration_max_ms,
            aspect_ratios,
            raw_footage_required,
            revision_limit,
            rights_requirements,
            status: "draft".into(),
            approved_at: None,
            created_at: now,
        };
        self.store.put(
            "brief",
            &brief.id,
            Some(&campaign_id),
            None,
            &brief.status,
            Some(&format!("{campaign_id}:{version}")),
            &brief,
            &brief.created_at,
        )?;
        self.audit(
            "brief",
            &brief.id,
            "created",
            json!({"campaign_id": campaign_id, "version": version}),
        )?;
        Ok(brief)
    }

    pub fn approve_brief(&self, id: &str) -> Result<Brief> {
        let mut brief: Brief = self.store.get("brief", id)?;
        ensure_transition("brief", &brief.status, "approved")?;
        brief.status = "approved".into();
        brief.approved_at = Some(Store::now());
        self.store.put(
            "brief",
            &brief.id,
            Some(&brief.campaign_id),
            None,
            &brief.status,
            Some(&format!("{}:{}", brief.campaign_id, brief.version)),
            &brief,
            &brief.created_at,
        )?;
        self.audit("brief", id, "approved", json!({"version": brief.version}))?;
        Ok(brief)
    }

    pub fn add_creator(
        &self,
        display_name: String,
        email: Option<String>,
        languages: Vec<String>,
        markets: Vec<String>,
        niches: Vec<String>,
        metadata: Value,
    ) -> Result<Creator> {
        required("display_name", &display_name)?;
        if let Some(candidate) = &email {
            let existing: Vec<Creator> = self.store.list("creator", None, None)?;
            if existing
                .iter()
                .any(|creator| creator.email.as_deref() == Some(candidate))
            {
                bail!("creator with email {candidate} already exists");
            }
        }
        let now = Store::now();
        let creator = Creator {
            id: Store::id(),
            display_name,
            email,
            languages,
            markets,
            niches,
            status: "active".into(),
            metadata,
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.put(
            "creator",
            &creator.id,
            None,
            None,
            &creator.status,
            creator.email.as_deref(),
            &creator,
            &creator.created_at,
        )?;
        self.audit("creator", &creator.id, "created", json!({}))?;
        Ok(creator)
    }

    pub fn add_creator_identity(
        &self,
        creator_id: String,
        connection_id: Option<String>,
        platform: String,
        external_creator_id: String,
        profile_url: Option<String>,
        metadata: Value,
    ) -> Result<CreatorIdentity> {
        let _: Creator = self.store.get("creator", &creator_id)?;
        if let Some(connection) = &connection_id {
            let _: Connection = self.store.get("connection", connection)?;
        }
        let identity = CreatorIdentity {
            id: Store::id(),
            creator_id: creator_id.clone(),
            connection_id,
            platform: platform.clone(),
            external_creator_id: external_creator_id.clone(),
            profile_url,
            metadata,
            last_synced_at: None,
        };
        let external = format!("{platform}:{external_creator_id}");
        self.store.put(
            "creator_identity",
            &identity.id,
            Some(&creator_id),
            identity.connection_id.as_deref(),
            "active",
            Some(&external),
            &identity,
            &Store::now(),
        )?;
        self.audit(
            "creator",
            &creator_id,
            "identity_added",
            json!({"platform": platform, "external_creator_id": external_creator_id}),
        )?;
        Ok(identity)
    }

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
    ) -> Result<Assignment> {
        let _: Campaign = self.store.get("campaign", &campaign_id)?;
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
        let now = Store::now();
        let assignment = Assignment {
            id: Store::id(),
            campaign_id: campaign_id.clone(),
            brief_id,
            creator_id,
            connection_id,
            external_assignment_id: None,
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
            None,
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

    pub fn add_submission(
        &self,
        assignment_id: String,
        external_submission_id: Option<String>,
    ) -> Result<Submission> {
        let assignment: Assignment = self.store.get("assignment", &assignment_id)?;
        if matches!(assignment.status.as_str(), "cancelled" | "completed") {
            bail!("cannot submit to assignment in {} state", assignment.status);
        }
        let existing: Vec<Submission> =
            self.store.list("submission", Some(&assignment_id), None)?;
        let revision = existing
            .iter()
            .map(|submission| submission.revision)
            .max()
            .unwrap_or("".len() as i64)
            + "r".len() as i64;
        let submission = Submission {
            id: Store::id(),
            assignment_id: assignment_id.clone(),
            external_submission_id,
            revision,
            status: "received".into(),
            feedback: None,
            qc_status: None,
            qc_report: None,
            submitted_at: Store::now(),
            reviewed_at: None,
            approved_at: None,
        };
        self.store.put(
            "submission",
            &submission.id,
            Some(&assignment_id),
            None,
            &submission.status,
            submission.external_submission_id.as_deref(),
            &submission,
            &submission.submitted_at,
        )?;
        self.audit(
            "submission",
            &submission.id,
            "received",
            json!({"assignment_id": assignment_id, "revision": revision}),
        )?;
        Ok(submission)
    }

    pub fn submission_review(
        &self,
        id: &str,
        status: &str,
        feedback: Option<String>,
    ) -> Result<Submission> {
        let mut submission: Submission = self.store.get("submission", id)?;
        ensure_transition("submission", &submission.status, status)?;
        submission.status = status.into();
        submission.feedback = feedback;
        submission.reviewed_at = Some(Store::now());
        if status == "approved" {
            submission.approved_at = Some(Store::now());
        }
        self.store.put(
            "submission",
            &submission.id,
            Some(&submission.assignment_id),
            None,
            &submission.status,
            submission.external_submission_id.as_deref(),
            &submission,
            &submission.submitted_at,
        )?;
        self.audit(
            "submission",
            id,
            "reviewed",
            json!({"status": status, "feedback": submission.feedback}),
        )?;
        if status == "revision_requested" {
            let assignment: Assignment = self.store.get("assignment", &submission.assignment_id)?;
            if let Some(connection) = assignment.connection_id {
                self.store.enqueue(
                    "request_revision",
                    "submission",
                    id,
                    Some(&connection),
                    &serde_json::to_value(&submission)?,
                )?;
            }
        }
        Ok(submission)
    }

    pub fn grant_rights(&self, rights: UsageRights) -> Result<UsageRights> {
        let _: Assignment = self.store.get("assignment", &rights.assignment_id)?;
        if let Some(asset_id) = &rights.asset_id {
            let _: Value = self.store.get("asset", asset_id)?;
        }
        self.store.put(
            "usage_rights",
            &rights.id,
            Some(&rights.assignment_id),
            rights.asset_id.as_deref(),
            "active",
            None,
            &rights,
            &rights.created_at,
        )?;
        self.audit(
            "assignment",
            &rights.assignment_id,
            "rights_granted",
            serde_json::to_value(&rights)?,
        )?;
        Ok(rights)
    }

    pub fn check_rights(
        &self,
        assignment_id: &str,
        channel: &str,
        paid: bool,
        at: Option<&str>,
    ) -> Result<Value> {
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
            if item
                .starts_at
                .parse::<DateTime<Utc>>()
                .map(|start| start > instant)
                .unwrap_or(false)
            {
                reasons.push("license has not started".to_string());
                continue;
            }
            if item
                .expires_at
                .as_deref()
                .and_then(|value| value.parse::<DateTime<Utc>>().ok())
                .map(|end| end <= instant)
                .unwrap_or(false)
            {
                reasons.push("license expired".to_string());
                continue;
            }
            if !item.channels.is_empty()
                && !item.channels.iter().any(|candidate| candidate == channel)
            {
                reasons.push(format!("channel {channel} not licensed"));
                continue;
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
        Ok(
            json!({"allowed": allowed, "assignment_id": assignment_id, "channel": channel, "paid": paid, "reasons": reasons}),
        )
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

    pub fn publish_campaign(
        &self,
        campaign_id: String,
        brief_id: String,
        connection_id: String,
    ) -> Result<Publication> {
        let campaign: Campaign = self.store.get("campaign", &campaign_id)?;
        let brief: Brief = self.store.get("brief", &brief_id)?;
        let _: Connection = self.store.get("connection", &connection_id)?;
        if brief.campaign_id != campaign_id || brief.status != "approved" {
            bail!("publication requires an approved brief belonging to the campaign");
        }
        if matches!(campaign.status.as_str(), "cancelled" | "completed") {
            bail!("campaign cannot be published in {} state", campaign.status);
        }
        let now = Store::now();
        let publication = Publication {
            id: Store::id(),
            campaign_id: campaign_id.clone(),
            brief_id: brief_id.clone(),
            connection_id: connection_id.clone(),
            external_campaign_id: None,
            external_url: None,
            status: "queued".into(),
            provider_status: None,
            last_synced_at: None,
            sync_error: None,
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.put(
            "publication",
            &publication.id,
            Some(&campaign_id),
            Some(&connection_id),
            &publication.status,
            None,
            &publication,
            &publication.created_at,
        )?;
        self.store.enqueue(
            "publish_campaign",
            "publication",
            &publication.id,
            Some(&connection_id),
            &json!({"campaign": campaign, "brief": brief, "publication": publication}),
        )?;
        self.audit("publication", &publication.id, "queued", json!({"campaign_id": campaign_id, "brief_id": brief_id, "connection_id": connection_id}))?;
        Ok(publication)
    }

    pub fn audit(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        action: &str,
        details: Value,
    ) -> Result<()> {
        self.store
            .audit(aggregate_type, aggregate_id, action, self.actor, &details)
    }
}

fn required(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} is required");
    }
    Ok(())
}

pub fn ensure_transition(kind: &str, from: &str, to: &str) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let allowed = match kind {
        "campaign" => matches!(
            (from, to),
            ("draft", "ready")
                | ("ready", "published")
                | ("published", "sourcing")
                | ("sourcing", "active")
                | ("active", "completed")
                | (_, "cancelled")
                | (_, "failed")
        ),
        "brief" => matches!(
            (from, to),
            ("draft", "approved") | ("draft", "archived") | ("approved", "archived")
        ),
        "assignment" => matches!(
            (from, to),
            ("invited", "applied")
                | ("invited", "accepted")
                | ("applied", "accepted")
                | ("accepted", "product_shipping")
                | ("accepted", "in_production")
                | ("product_shipping", "in_production")
                | ("in_production", "submitted")
                | ("submitted", "revision_requested")
                | ("revision_requested", "in_production")
                | ("submitted", "approved")
                | ("approved", "licensed")
                | ("licensed", "paid")
                | ("paid", "completed")
                | (_, "cancelled")
                | (_, "failed")
        ),
        "shipment" => matches!(
            (from, to),
            ("awaiting_address", "ready_to_ship")
                | ("ready_to_ship", "shipped")
                | ("shipped", "delivered")
                | (_, "failed")
                | (_, "cancelled")
        ),
        "submission" => matches!(
            (from, to),
            ("received", "ingesting")
                | ("received", "qc_pending")
                | ("ingesting", "qc_pending")
                | ("qc_pending", "pending_review")
                | ("pending_review", "approved")
                | ("pending_review", "rejected")
                | ("pending_review", "revision_requested")
                | ("received", "pending_review")
                | ("received", "rejected")
        ),
        "payment" => matches!(
            (from, to),
            ("pending", "processing")
                | ("processing", "paid")
                | ("external_pending", "paid")
                | ("failed", "pending")
                | (_, "failed")
                | (_, "cancelled")
                | ("paid", "refunded")
                | ("paid", "disputed")
        ),
        _ => false,
    };
    if !allowed {
        bail!("invalid {kind} transition: {from} -> {to}");
    }
    Ok(())
}
