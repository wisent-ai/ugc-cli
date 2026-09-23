use super::*;

impl<'a> StandaloneService<'a> {
    /// Moves the campaign through draft, ready, published, sourcing and active
    /// as far as its evidence allows, recording each step as an action or a
    /// blocker; with `apply` the status is written as well.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn advance_campaign_phase(
        &self,
        service: &UgcService<'_>,
        campaign_id: &str,
        campaign: &mut Campaign,
        approved_brief: bool,
        conversations: &[Conversation],
        assignments: &[Assignment],
        apply: bool,
        actions: &mut Vec<String>,
        blockers: &mut Vec<String>,
    ) -> Result<()> {
    if campaign.status == "draft" {
        if approved_brief {
            actions.push("campaign draft -> ready".into());
            if apply {
                *campaign = service.campaign_status(campaign_id, "ready")?;
            }
        } else {
            blockers.push("approve at least one brief".into());
        }
    }
    if campaign.status == "ready" {
        if conversations.is_empty() {
            blockers.push("start creator outreach".into());
        } else {
            actions.push("campaign ready -> published".into());
            if apply {
                *campaign = service.campaign_status(campaign_id, "published")?;
            }
        }
    }
    if campaign.status == "published" {
        actions.push("campaign published -> sourcing".into());
        if apply {
            *campaign = service.campaign_status(campaign_id, "sourcing")?;
        }
    }
    if campaign.status == "sourcing" {
        if assignments.iter().any(|assignment| {
            !matches!(
                assignment.status.as_str(),
                "invited" | "applied" | "cancelled" | "failed"
            )
        }) {
            actions.push("campaign sourcing -> active".into());
            if apply {
                *campaign = service.campaign_status(campaign_id, "active")?;
            }
        } else {
            blockers.push("obtain at least one accepted creator assignment".into());
        }
    }
        Ok(())
    }
}
