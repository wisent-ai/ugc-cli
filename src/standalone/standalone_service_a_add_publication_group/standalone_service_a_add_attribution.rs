use super::*;

impl<'a> StandaloneService<'a> {
    pub fn add_attribution(
        &self,
        publication_id: &str,
        event_type: String,
        external_event_id: Option<String>,
        value_minor: Option<i64>,
        currency: Option<String>,
        metadata: Value,
        occurred_at: Option<String>,
    ) -> Result<AttributionEvent> {
        let publication: StandalonePublication =
            self.store.get("standalone_publication", publication_id)?;
        if event_type.trim().is_empty() {
            bail!("attribution event type is required");
        }
        if value_minor.is_some_and(|value| value < 0) {
            bail!("attribution value cannot be negative");
        }
        if value_minor.is_some() != currency.is_some() {
            bail!("value and currency must be supplied together");
        }
        if let Some(currency) = &currency {
            let campaign: Campaign = self.store.get("campaign", &publication.campaign_id)?;
            if !currency.eq_ignore_ascii_case(&campaign.currency) {
                bail!("attribution currency must match campaign currency");
            }
        }
        if currency
            .as_deref()
            .is_some_and(|currency| currency.trim().is_empty())
        {
            bail!("attribution currency cannot be empty");
        }
        if let Some(external_event_id) = external_event_id.as_deref() {
            if let Some(existing) = self
                .store
                .find_external::<AttributionEvent>("attribution_event", external_event_id)?
            {
                let same_event = existing.publication_id == publication_id
                    && existing.event_type.eq_ignore_ascii_case(&event_type)
                    && existing.value_minor == value_minor
                    && existing.currency == currency
                    && existing.metadata == metadata;
                if same_event {
                    return Ok(existing);
                }
                bail!("external attribution event ID was already used for a different event");
            }
        }
        let occurred_at = occurred_at.unwrap_or_else(Store::now);
        DateTime::parse_from_rfc3339(&occurred_at)
            .context("invalid attribution occurrence timestamp")?;
        let now = Store::now();
        let event = AttributionEvent {
            id: Store::id(),
            publication_id: publication_id.into(),
            event_type,
            external_event_id: external_event_id.clone(),
            value_minor,
            currency,
            metadata,
            occurred_at,
            created_at: now.clone(),
        };
        self.store.put(
            "attribution_event",
            &event.id,
            Some(publication_id),
            None,
            "recorded",
            external_event_id.as_deref(),
            &event,
            &now,
        )?;
        self.audit(
            "standalone_publication",
            publication_id,
            "attribution_recorded",
            json!({"event_id": event.id}),
        )?;
        Ok(event)
    }
    pub fn performance_report(&self, campaign_id: &str) -> Result<Value> {
        let campaign: Campaign = self.store.get("campaign", campaign_id)?;
        let publications: Vec<StandalonePublication> =
            self.store
                .list("standalone_publication", Some(campaign_id), None)?;
        let assignments: Vec<Assignment> =
            self.store.list("assignment", Some(campaign_id), None)?;
        let mut totals = MetricTotals::default();
        let mut rows = Vec::new();
        let mut attributed_revenue_minor: i64 = 0;
        let mut attributed_conversions: i64 = 0;
        let mut attributed_events: i64 = 0;
        for publication in &publications {
            let snapshots: Vec<MetricSnapshot> =
                self.store
                    .list("metric_snapshot", Some(&publication.id), None)?;
            let latest = snapshots.first().cloned();
            if let Some(snapshot) = &latest {
                totals.add(snapshot);
            }
            let attribution: Vec<AttributionEvent> =
                self.store
                    .list("attribution_event", Some(&publication.id), None)?;
            attributed_events +=
                i64::try_from(attribution.len()).context("attribution count overflow")?;
            for event in &attribution {
                attributed_revenue_minor += event.value_minor.unwrap_or_default();
                if is_conversion_event(&event.event_type) {
                    attributed_conversions += 1;
                }
            }
            rows.push(
                json!({"publication": publication, "latest": latest, "attribution": attribution}),
            );
        }
        let creator_cost_minor: i64 = assignments
            .iter()
            .filter_map(|assignment| assignment.compensation_minor)
            .sum();
        let engagement = totals.likes + totals.comments + totals.shares + totals.saves;
        let engagement_rate = ratio(engagement, totals.views);
        let click_rate = ratio(totals.clicks, totals.views);
        let canonical_conversions = if attributed_conversions > 0 {
            attributed_conversions
        } else {
            totals.conversions
        };
        let canonical_revenue_minor = if attributed_revenue_minor > 0 {
            attributed_revenue_minor
        } else {
            totals.revenue_minor
        };
        let conversion_rate = ratio(canonical_conversions, totals.clicks);
        let tracked_cost = creator_cost_minor + totals.spend_minor;
        let roas = ratio(canonical_revenue_minor, tracked_cost);
        Ok(json!({
            "campaign": campaign,
            "publications": rows,
            "totals": {
                "views": totals.views, "likes": totals.likes, "comments": totals.comments,
                "shares": totals.shares, "saves": totals.saves, "clicks": totals.clicks,
                "conversions": canonical_conversions, "revenue_minor": canonical_revenue_minor,
                "media_spend_minor": totals.spend_minor, "creator_cost_minor": creator_cost_minor,
                "tracked_cost_minor": tracked_cost,
                "metric_revenue_minor": totals.revenue_minor,
                "attributed_revenue_minor": attributed_revenue_minor,
                "attributed_conversions": attributed_conversions,
                "attribution_events": attributed_events,
            },
            "rates": {"engagement_rate": engagement_rate, "click_rate": click_rate, "conversion_rate": conversion_rate, "roas": roas},
            "currency": campaign.currency,
            "generated_at": Store::now(),
        }))
    }
}
