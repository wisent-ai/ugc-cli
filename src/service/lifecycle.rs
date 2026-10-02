//! The counterparts of adding a brief and adding a creator. A brief is
//! retired, not deleted: assignments and publications name its version, so it
//! goes to `archived`, the status the brief lifecycle already allows. A
//! creator nobody has assigned work to is removed with its identities; one
//! with assignments is refused with them named.

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
}
