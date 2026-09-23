use super::*;

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
        if let Some(deadline) = deadline.as_deref() {
            let deadline =
                DateTime::parse_from_rfc3339(deadline).context("invalid campaign deadline")?;
            if deadline <= Utc::now() {
                bail!("campaign deadline must be in the future");
            }
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
        // The ledger accepted a campaign record, which is this product's
        // first real result: the first-use fact is stamped here, where the
        // effect happens, not where a walkthrough asks about it.
        crate::onboarding::record_first_success(self.store, &campaign)?;
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
            .unwrap_or(0)
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
}
