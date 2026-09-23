use super::*;

/// A portal link a creator receives stays valid this long when the caller sets no other span.
pub(crate) const PORTAL_VALIDITY_DAYS: i64 = 30;

/// How many creators `discover` returns when the query sets no limit.
pub(crate) const DEFAULT_DISCOVERY_LIMIT: usize = 20;

// Discovery scoring: every creator starts at the base, each matched filter
// adds its weight, each evidence signal adds the bonus, and the total is capped.
pub(crate) const BASE_MATCH_SCORE: i64 = 10;

pub(crate) const MARKET_WEIGHT: i64 = 20;

pub(crate) const LANGUAGE_WEIGHT: i64 = 20;

pub(crate) const NICHE_WEIGHT: i64 = 25;

pub(crate) const CHANNEL_WEIGHT: i64 = 10;

pub(crate) const SIGNAL_BONUS: i64 = 5;

pub(crate) const MAX_MATCH_SCORE: i64 = 100;

/// An engagement rate at or above this counts as evidence of an audience.
pub(crate) const STRONG_ENGAGEMENT_RATE: f64 = 0.03;

/// A response rate at or above this counts as a creator who answers.
pub(crate) const RESPONSIVE_RATE: f64 = 0.5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorSeed {
    pub display_name: String,
    pub email: Option<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub markets: Vec<String>,
    #[serde(default)]
    pub niches: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub identities: Vec<IdentitySeed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySeed {
    pub platform: String,
    pub external_creator_id: String,
    pub profile_url: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInput {
    pub views: i64,
    pub likes: i64,
    pub comments: i64,
    pub shares: i64,
    pub saves: i64,
    pub clicks: i64,
    pub conversions: i64,
    pub revenue_minor: i64,
    pub spend_minor: i64,
    pub currency: String,
    pub source: String,
    pub captured_at: Option<String>,
}

pub struct StandaloneService<'a> {
    pub store: &'a Store,
    pub actor: &'a str,
}

impl<'a> StandaloneService<'a> {
    pub fn import_creators(&self, seeds: Vec<CreatorSeed>) -> Result<Value> {
        let service = self.core();
        let mut created = Vec::new();
        let mut existing = Vec::new();
        for seed in seeds {
            let duplicate = seed.email.as_deref().and_then(|email| {
                self.store
                    .list::<Creator>("creator", None, None)
                    .ok()?
                    .into_iter()
                    .find(|creator| {
                        creator
                            .email
                            .as_deref()
                            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(email))
                    })
            });
            let creator = match duplicate {
                Some(creator) => {
                    existing.push(creator.id.clone());
                    creator
                }
                None => {
                    let creator = service.add_creator(
                        seed.display_name,
                        seed.email,
                        seed.languages,
                        seed.markets,
                        seed.niches,
                        seed.metadata,
                    )?;
                    created.push(creator.id.clone());
                    creator
                }
            };
            for identity in seed.identities {
                let identities: Vec<CreatorIdentity> =
                    self.store
                        .list("creator_identity", Some(&creator.id), None)?;
                if identities.iter().any(|item| {
                    item.platform == identity.platform
                        && item.external_creator_id == identity.external_creator_id
                }) {
                    continue;
                }
                service.add_creator_identity(
                    creator.id.clone(),
                    None,
                    identity.platform,
                    identity.external_creator_id,
                    identity.profile_url,
                    identity.metadata,
                )?;
            }
        }
        Ok(json!({"created": created, "existing": existing}))
    }
    pub fn register_creator(&self, seed: CreatorSeed, portal_days: Option<i64>) -> Result<Value> {
        let email = seed
            .email
            .as_deref()
            .map(str::trim)
            .filter(|email| !email.is_empty())
            .context("self-registration requires an email")?;
        let creators: Vec<Creator> = self.store.list("creator", None, None)?;
        if creators.iter().any(|creator| {
            creator
                .email
                .as_deref()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(email))
        }) {
            bail!("a creator with this email already exists; an operator must issue portal access");
        }
        let mut identities = Vec::with_capacity(seed.identities.len());
        let mut identity_keys = BTreeSet::new();
        for mut identity in seed.identities {
            identity.platform = identity.platform.trim().to_ascii_lowercase();
            identity.external_creator_id = identity.external_creator_id.trim().to_string();
            if identity.platform.is_empty() || identity.external_creator_id.is_empty() {
                bail!("self-reported identity platform and external creator ID are required");
            }
            identity.profile_url = identity
                .profile_url
                .map(|url| url.trim().to_string())
                .filter(|url| !url.is_empty());
            let key = format!("{}:{}", identity.platform, identity.external_creator_id);
            if !identity_keys.insert(key.clone()) {
                bail!("self-registration contains a duplicate identity: {key}");
            }
            if self
                .store
                .find_external::<CreatorIdentity>("creator_identity", &key)?
                .is_some()
            {
                bail!("creator identity is already registered: {key}");
            }
            identities.push(identity);
        }
        let metadata = json!({
            "verification_status": "unverified",
            "self_reported": seed.metadata,
            "self_reported_identities": identities.clone(),
        });
        let service = self.core();
        let creator = service.add_creator(
            seed.display_name,
            seed.email,
            seed.languages,
            seed.markets,
            seed.niches,
            metadata,
        )?;
        let mut created_identities = Vec::with_capacity(identities.len());
        for identity in identities {
            let metadata = json!({
                "verification_status": "unverified",
                "self_reported": identity.metadata,
            });
            created_identities.push(service.add_creator_identity(
                creator.id.clone(),
                None,
                identity.platform,
                identity.external_creator_id,
                identity.profile_url,
                metadata,
            )?);
        }
        let portal = self.create_portal_access(&creator.id, portal_days.or(Some(PORTAL_VALIDITY_DAYS)))?;
        self.audit("creator", &creator.id, "self_registered", json!({}))?;
        Ok(json!({
            "creator": creator,
            "identities": created_identities,
            "portal": portal,
            "identity_status": "pending_operator_verification",
        }))
    }
}
