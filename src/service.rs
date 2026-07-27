use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::{
    db::Store,
    model::{
        Asset, Assignment, Brief, Campaign, Connection, Creator, CreatorIdentity, Message, Payment,
        ProviderCapabilities, Publication, Shipment, ShippingAddress, Submission, UsageRights,
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
        required("currency", &currency)?;
        if budget_minor.is_some_and(|budget| budget < i64::default()) {
            bail!("campaign budget cannot be negative");
        }
        let currency = currency.trim().to_ascii_uppercase();
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
        required("service_type", &service_type)?;
        required("creative_angle", &creative_angle)?;
        if duration_min_ms.is_some_and(|duration| duration < i64::default())
            || duration_max_ms.is_some_and(|duration| duration < i64::default())
        {
            bail!("brief durations cannot be negative");
        }
        if duration_min_ms
            .zip(duration_max_ms)
            .is_some_and(|(minimum, maximum)| minimum > maximum)
        {
            bail!("brief minimum duration cannot exceed maximum duration");
        }
        if revision_limit.is_some_and(|limit| limit < i64::default()) {
            bail!("brief revision limit cannot be negative");
        }
        if !rights_requirements.is_object() {
            bail!("brief rights requirements must be a JSON object");
        }
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
        let email = email.map(|candidate| candidate.trim().to_ascii_lowercase());
        if email.as_deref().is_some_and(str::is_empty) {
            bail!("creator email cannot be empty");
        }
        required("display_name", &display_name)?;
        if let Some(candidate) = &email {
            let existing: Vec<Creator> = self.store.list("creator", None, None)?;
            if existing.iter().any(|creator| {
                creator
                    .email
                    .as_deref()
                    .is_some_and(|email| email.eq_ignore_ascii_case(candidate))
            }) {
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

    pub fn verify_creator(&self, id: &str, verified_metadata: Value) -> Result<Creator> {
        let mut creator: Creator = self.store.get("creator", id)?;
        let Value::Object(mut metadata) = verified_metadata else {
            bail!("verified creator metadata must be a JSON object");
        };
        if let Some(self_reported) = creator.metadata.get("self_reported").cloned() {
            metadata.entry("self_reported").or_insert(self_reported);
        }
        if let Some(self_reported_identities) =
            creator.metadata.get("self_reported_identities").cloned()
        {
            metadata
                .entry("self_reported_identities")
                .or_insert(self_reported_identities);
        }
        metadata.insert(
            "verification_status".into(),
            Value::String("verified".into()),
        );
        creator.metadata = Value::Object(metadata);
        creator.updated_at = Store::now();
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
        let identities: Vec<CreatorIdentity> =
            self.store.list("creator_identity", Some(id), None)?;
        for mut identity in identities {
            if identity
                .metadata
                .get("verification_status")
                .and_then(Value::as_str)
                != Some("unverified")
            {
                continue;
            }
            let mut metadata = match std::mem::take(&mut identity.metadata) {
                Value::Object(metadata) => metadata,
                self_reported => {
                    let mut metadata = serde_json::Map::new();
                    metadata.insert("self_reported".into(), self_reported);
                    metadata
                }
            };
            metadata.insert(
                "verification_status".into(),
                Value::String("verified".into()),
            );
            metadata.insert("verified_at".into(), Value::String(Store::now()));
            identity.metadata = Value::Object(metadata);
            let external = format!("{}:{}", identity.platform, identity.external_creator_id);
            self.store.put(
                "creator_identity",
                &identity.id,
                Some(&identity.creator_id),
                identity.connection_id.as_deref(),
                "active",
                Some(&external),
                &identity,
                &Store::now(),
            )?;
        }
        self.audit("creator", id, "verified", json!({}))?;
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
        required("platform", &platform)?;
        required("external_creator_id", &external_creator_id)?;
        let platform = platform.trim().to_ascii_lowercase();
        let external_creator_id = external_creator_id.trim().to_string();
        let profile_url = profile_url
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty());
        let _: Creator = self.store.get("creator", &creator_id)?;
        if let Some(connection) = &connection_id {
            let _: Connection = self.store.get("connection", connection)?;
        }
        let external = format!("{platform}:{external_creator_id}");
        if let Some(existing) = self
            .store
            .find_external::<CreatorIdentity>("creator_identity", &external)?
        {
            let same_identity = existing.creator_id == creator_id
                && existing.connection_id == connection_id
                && existing.profile_url == profile_url
                && existing.metadata == metadata;
            if same_identity {
                return Ok(existing);
            }
            bail!("creator identity is already registered: {external}");
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

    pub fn add_submission(
        &self,
        assignment_id: String,
        external_submission_id: Option<String>,
    ) -> Result<Submission> {
        if let Some(external_id) = external_submission_id.as_deref() {
            if let Some(existing) = self
                .store
                .find_external::<Submission>("submission", external_id)?
            {
                if existing.assignment_id == assignment_id {
                    return Ok(existing);
                }
                bail!("external submission ID was already used for another assignment");
            }
        }
        let assignment: Assignment = self.store.get("assignment", &assignment_id)?;
        if assignment.status != "in_production" {
            bail!("assignment must be in production before submission");
        }
        let existing: Vec<Submission> =
            self.store.list("submission", Some(&assignment_id), None)?;
        let revision = existing
            .iter()
            .map(|submission| submission.revision)
            .max()
            .unwrap_or("".len() as i64)
            + "r".len() as i64;
        if let Some(limit) = assignment.revision_limit {
            let Some(maximum_revision) = limit.checked_add("r".len() as i64) else {
                bail!("assignment revision limit overflow");
            };
            if revision > maximum_revision {
                bail!("assignment revision limit has been reached");
            }
        }
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
        if status == "approved" && !matches!(submission.qc_status.as_deref(), Some("PASS" | "WARN"))
        {
            bail!("submission cannot be approved before technical QC passes");
        }
        if matches!(status, "revision_requested" | "rejected")
            && feedback
                .as_deref()
                .is_none_or(|feedback| feedback.trim().is_empty())
        {
            bail!("review feedback is required for revision or rejection");
        }
        if status == "revision_requested" {
            let assignment: Assignment = self.store.get("assignment", &submission.assignment_id)?;
            if assignment
                .revision_limit
                .is_some_and(|limit| submission.revision > limit)
            {
                bail!("assignment revision limit has been reached");
            }
        }
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
            self.assignment_status(&assignment.id, "revision_requested")?;
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
        if status == "rejected" {
            self.assignment_status(&submission.assignment_id, "failed")?;
        }
        Ok(submission)
    }

    pub fn grant_rights(&self, rights: UsageRights) -> Result<UsageRights> {
        let _: Assignment = self.store.get("assignment", &rights.assignment_id)?;
        required("rights owner", &rights.owner)?;
        required("license type", &rights.license_type)?;
        let starts_at = DateTime::parse_from_rfc3339(&rights.starts_at)?.with_timezone(&Utc);
        if let Some(expires_at) = &rights.expires_at {
            let expires_at = DateTime::parse_from_rfc3339(expires_at)?.with_timezone(&Utc);
            if expires_at <= starts_at {
                bail!("usage rights expiration must be after start");
            }
        }
        if let Some(asset_id) = &rights.asset_id {
            let asset: Asset = self.store.get("asset", asset_id)?;
            let Some(submission_id) = asset.submission_id else {
                bail!("rights asset must belong to a submission");
            };
            let submission: Submission = self.store.get("submission", &submission_id)?;
            if submission.assignment_id != rights.assignment_id {
                bail!("rights asset does not belong to assignment");
            }
            if submission.status != "approved" {
                bail!("rights asset submission must be approved");
            }
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
