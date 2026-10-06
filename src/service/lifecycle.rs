//! The counterparts of what the setup commands add: changing a campaign, a
//! creator or a connection, retiring a brief, removing a creator, an
//! identity or a connection, and revoking usage rights. A brief is retired,
//! not deleted: assignments and publications name its version, so it goes to
//! `archived`, the status the brief lifecycle already allows. A creator or a
//! connection something still names is refused with the holders named.
//! Revoked rights are kept as the record of what was licensed and stop
//! counting in every check.

use super::*;

impl<'a> UgcService<'a> {
    /// The counterpart of `create_campaign` for what a campaign says about
    /// itself. Removal is `campaign status <id> cancelled`, which the
    /// lifecycle allows from any state; brand, product and currency stay, as
    /// briefs and payments were written against them.
    pub fn edit_campaign(
        &self,
        id: &str,
        name: Option<String>,
        objective: Option<String>,
        deadline: Option<String>,
        budget_minor: Option<i64>,
    ) -> Result<Campaign> {
        if name.is_none() && objective.is_none() && deadline.is_none() && budget_minor.is_none() {
            bail!("campaign edit needs at least one of --name, --objective, --deadline, --budget-minor");
        }
        let mut campaign: Campaign = self.store.get("campaign", id)?;
        if let Some(name) = name {
            campaign.name = name;
        }
        if let Some(objective) = objective {
            campaign.objective = objective;
        }
        if deadline.is_some() {
            campaign.deadline = deadline;
        }
        if budget_minor.is_some() {
            campaign.budget_minor = budget_minor;
        }
        campaign.updated_at = Store::now();
        self.store.put("campaign", &campaign.id, None, None, &campaign.status, None, &campaign, &campaign.created_at)?;
        self.audit("campaign", id, "edited", json!({}))?;
        Ok(campaign)
    }
    pub fn archive_brief(&self, id: &str) -> Result<Brief> {
        let mut brief: Brief = self.store.get("brief", id)?;
        if brief.status == "archived" {
            bail!("brief {id} is already archived");
        }
        ensure_transition("brief", &brief.status, "archived")?;
        brief.status = "archived".into();
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
        self.audit("brief", id, "archived", json!({"version": brief.version}))?;
        Ok(brief)
    }

    pub fn remove_creator(&self, id: &str) -> Result<Creator> {
        let creator: Creator = self.store.get("creator", id)?;
        let assignments: Vec<Assignment> = self.store.list("assignment", None, None)?;
        let held: Vec<String> = assignments
            .iter()
            .filter(|assignment| assignment.creator_id == id)
            .map(|assignment| assignment.id.clone())
            .collect();
        if !held.is_empty() {
            bail!(
                "creator {id} cannot be removed while assignments name them: {}; finish or cancel those first",
                held.join(", ")
            );
        }
        let identities: Vec<CreatorIdentity> = self.store.list("creator_identity", Some(id), None)?;
        for identity in &identities {
            self.store.delete("creator_identity", &identity.id)?;
        }
        self.store.delete("creator", id)?;
        self.audit("creator", id, "removed", json!({"identities": identities.len()}))?;
        Ok(creator)
    }

    /// The counterpart of `grant_rights`: the grant stays as the record of
    /// what was licensed, its status becomes `revoked`, and `rights check`,
    /// release and the campaign workflow no longer count it.
    pub fn revoke_rights(&self, id: &str, reason: &str) -> Result<UsageRights> {
        if reason.trim().is_empty() {
            bail!("--reason must say why the rights are revoked, such as the contract that ended them");
        }
        let record = self.store.get_record("usage_rights", id)?;
        if record.status == "revoked" {
            bail!("usage rights {id} are already revoked");
        }
        let rights: UsageRights = serde_json::from_value(record.data.clone())?;
        self.store.put(
            "usage_rights",
            &rights.id,
            Some(&rights.assignment_id),
            rights.asset_id.as_deref(),
            "revoked",
            None,
            &rights,
            &rights.created_at,
        )?;
        self.audit("assignment", &rights.assignment_id, "rights_revoked", json!({"rights_id": rights.id, "reason": reason}))?;
        Ok(rights)
    }

    /// Change what is known of a creator: name, email, languages, markets,
    /// niches. Identities and assignments stay.
    pub fn edit_creator(&self, id: &str, name: Option<String>, email: Option<String>, languages: Option<Vec<String>>, markets: Option<Vec<String>>, niches: Option<Vec<String>>) -> Result<Creator> {
        if name.is_none() && email.is_none() && languages.is_none() && markets.is_none() && niches.is_none() {
            bail!("creator edit needs at least one of --name, --email, --languages, --markets, --niches");
        }
        let record = self.store.get_record("creator", id)?;
        let mut creator: Creator = serde_json::from_value(record.data.clone())?;
        if let Some(name) = name {
            creator.display_name = name;
        }
        if email.is_some() {
            creator.email = email;
        }
        if let Some(languages) = languages {
            creator.languages = languages;
        }
        if let Some(markets) = markets {
            creator.markets = markets;
        }
        if let Some(niches) = niches {
            creator.niches = niches;
        }
        creator.updated_at = Store::now();
        self.store.put("creator", &creator.id, record.parent_id.as_deref(), record.secondary_id.as_deref(), &record.status, record.external_id.as_deref(), &creator, &record.created_at)?;
        self.audit("creator", id, "edited", json!({}))?;
        Ok(creator)
    }

    /// Take back `creator identity`: one platform identity of a creator.
    pub fn remove_creator_identity(&self, id: &str) -> Result<CreatorIdentity> {
        let identity: CreatorIdentity = self.store.get("creator_identity", id)?;
        self.store.delete("creator_identity", id)?;
        self.audit("creator", &identity.creator_id, "identity_removed", json!({"identity_id": id}))?;
        Ok(identity)
    }

    /// Change what a connection reaches and how: its name, base URL, the
    /// token and webhook-secret sources, the external account. Its provider,
    /// capabilities and sync cursor stay.
    pub fn edit_connection(&self, id: &str, name: Option<String>, base_url: Option<String>, token_source: Option<String>, webhook_secret_source: Option<String>, external_account_id: Option<String>) -> Result<Connection> {
        if name.is_none() && base_url.is_none() && token_source.is_none() && webhook_secret_source.is_none() && external_account_id.is_none() {
            bail!("connection edit needs at least one of --name, --base-url, --token-source, --webhook-secret-source, --external-account-id");
        }
        let mut connection: Connection = self.store.get("connection", id)?;
        if let Some(name) = name {
            connection.name = name;
        }
        if base_url.is_some() {
            connection.base_url = base_url;
        }
        if token_source.is_some() {
            connection.token_env = token_source;
        }
        if webhook_secret_source.is_some() {
            connection.webhook_secret_env = webhook_secret_source;
        }
        if external_account_id.is_some() {
            connection.external_account_id = external_account_id;
        }
        connection.updated_at = Store::now();
        self.store.put("connection", &connection.id, None, None, &connection.status, None, &connection, &connection.created_at)?;
        self.audit("connection", id, "edited", json!({}))?;
        Ok(connection)
    }

    /// The counterpart of `add_connection`. Refused while an assignment, a
    /// creator identity or a publication names the connection, since each
    /// would be left pointing at nothing; the refusal names them.
    pub fn remove_connection(&self, id: &str) -> Result<Connection> {
        let connection: Connection = self.store.get("connection", id)?;
        let mut held: Vec<String> = Vec::new();
        for assignment in self.store.list::<Assignment>("assignment", None, None)? {
            if assignment.connection_id.as_deref() == Some(id) {
                held.push(format!("assignment {}", assignment.id));
            }
        }
        for identity in self.store.list::<CreatorIdentity>("creator_identity", None, None)? {
            if identity.connection_id.as_deref() == Some(id) {
                held.push(format!("creator identity {}", identity.id));
            }
        }
        for publication in self.store.list::<Publication>("publication", None, None)? {
            if publication.connection_id == id {
                held.push(format!("publication {}", publication.id));
            }
        }
        if !held.is_empty() {
            bail!("connection {id} cannot be removed while these name it: {}; remove or reassign them first", held.join(", "));
        }
        self.store.delete("connection", id)?;
        self.audit("connection", id, "removed", json!({}))?;
        Ok(connection)
    }
}