use super::*;

impl<'a> UgcService<'a> {
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
}
