use super::*;

impl<'a> UgcService<'a> {
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

pub(crate) fn required(name: &str, value: &str) -> Result<()> {
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
