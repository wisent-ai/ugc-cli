use super::*;

impl<'a> StandaloneService<'a> {
    pub fn add_publication(
        &self,
        assignment_id: &str,
        submission_id: &str,
        asset_id: &str,
        platform: String,
        channel: String,
        territory: Option<String>,
        post_id: Option<String>,
        url: String,
        paid: bool,
        published_at: Option<String>,
    ) -> Result<StandalonePublication> {
        if url.trim().is_empty() {
            bail!("publication URL is required");
        }
        if platform.trim().is_empty() || channel.trim().is_empty() {
            bail!("publication platform and channel are required");
        }
        if post_id
            .as_deref()
            .is_some_and(|post_id| post_id.trim().is_empty())
        {
            bail!("publication post ID cannot be empty");
        }
        if territory
            .as_deref()
            .is_some_and(|territory| territory.trim().is_empty())
        {
            bail!("publication territory cannot be empty");
        }
        let assignment: Assignment = self.store.get("assignment", assignment_id)?;
        let submission: Submission = self.store.get("submission", submission_id)?;
        if submission.assignment_id != assignment.id || submission.status != "approved" {
            bail!("publication requires an approved assignment submission");
        }
        let asset: Asset = self.store.get("asset", asset_id)?;
        if asset.submission_id.as_deref() != Some(submission_id) {
            bail!("asset does not belong to submission");
        }
        let rights = self.core().check_rights(
            assignment_id,
            Some(asset_id),
            &channel,
            territory.as_deref(),
            paid,
            published_at.as_deref(),
        )?;
        if rights.get("allowed").and_then(Value::as_bool) != Some(true) {
            bail!("publication blocked by usage rights: {rights}");
        }
        let tracking_code = Uuid::new_v4().simple().to_string();
        let now = Store::now();
        let external_id = post_id
            .as_ref()
            .map(|post_id| format!("{}:{post_id}", platform.to_ascii_lowercase()));
        if let Some(external_id) = external_id.as_deref() {
            if let Some(existing) = self
                .store
                .find_external::<StandalonePublication>("standalone_publication", external_id)?
            {
                let same_publication = existing.assignment_id == assignment_id
                    && existing.submission_id == submission_id
                    && existing.asset_id == asset_id
                    && existing.channel.eq_ignore_ascii_case(&channel)
                    && existing.territory == territory
                    && existing.url == url
                    && existing.paid == paid;
                if same_publication {
                    return Ok(existing);
                }
                bail!("platform post ID was already used for a different publication");
            }
        }
        let publication = StandalonePublication {
            id: Store::id(),
            campaign_id: assignment.campaign_id.clone(),
            assignment_id: assignment.id.clone(),
            creator_id: assignment.creator_id,
            submission_id: submission.id,
            asset_id: asset.id,
            platform,
            channel,
            territory,
            post_id,
            url,
            tracking_code,
            paid,
            status: "active".into(),
            published_at: published_at.unwrap_or_else(Store::now),
            last_checked_at: None,
            created_at: now.clone(),
        };
        self.store.put(
            "standalone_publication",
            &publication.id,
            Some(&publication.campaign_id),
            Some(&publication.assignment_id),
            &publication.status,
            external_id.as_deref(),
            &publication,
            &now,
        )?;
        self.audit(
            "standalone_publication",
            &publication.id,
            "created",
            json!({"assignment_id": assignment_id, "asset_id": asset_id}),
        )?;
        Ok(publication)
    }
    pub fn capture_metrics(
        &self,
        publication_id: &str,
        input: MetricInput,
    ) -> Result<MetricSnapshot> {
        let mut publication: StandalonePublication =
            self.store.get("standalone_publication", publication_id)?;
        let campaign: Campaign = self.store.get("campaign", &publication.campaign_id)?;
        if input.currency.trim().is_empty() || input.source.trim().is_empty() {
            bail!("metric currency and source are required");
        }
        if !input.currency.eq_ignore_ascii_case(&campaign.currency) {
            bail!("metric currency must match campaign currency");
        }
        let counters = [
            input.views,
            input.likes,
            input.comments,
            input.shares,
            input.saves,
            input.clicks,
            input.conversions,
            input.revenue_minor,
            input.spend_minor,
        ];
        if counters.iter().any(|value| *value < 0) {
            bail!("metric counters cannot be negative");
        }
        let previous: Vec<MetricSnapshot> =
            self.store
                .list("metric_snapshot", Some(publication_id), None)?;
        if let Some(previous) = previous.first() {
            let decreased = input.views < previous.views
                || input.likes < previous.likes
                || input.comments < previous.comments
                || input.shares < previous.shares
                || input.saves < previous.saves
                || input.clicks < previous.clicks
                || input.conversions < previous.conversions
                || input.revenue_minor < previous.revenue_minor
                || input.spend_minor < previous.spend_minor;
            if decreased {
                bail!("cumulative metrics cannot decrease; record a correction event instead");
            }
            if !previous.currency.eq_ignore_ascii_case(&input.currency) {
                bail!("metric currency cannot change");
            }
        }
        let captured_at = input.captured_at.unwrap_or_else(Store::now);
        DateTime::parse_from_rfc3339(&captured_at).context("invalid metric capture timestamp")?;
        let snapshot = MetricSnapshot {
            id: Store::id(),
            publication_id: publication_id.into(),
            captured_at: captured_at.clone(),
            views: input.views,
            likes: input.likes,
            comments: input.comments,
            shares: input.shares,
            saves: input.saves,
            clicks: input.clicks,
            conversions: input.conversions,
            revenue_minor: input.revenue_minor,
            spend_minor: input.spend_minor,
            currency: input.currency,
            source: input.source,
        };
        self.store.put(
            "metric_snapshot",
            &snapshot.id,
            Some(publication_id),
            None,
            "captured",
            None,
            &snapshot,
            &captured_at,
        )?;
        publication.last_checked_at = Some(captured_at);
        let external_id = publication
            .post_id
            .as_ref()
            .map(|post_id| format!("{}:{post_id}", publication.platform.to_ascii_lowercase()));
        self.store.put(
            "standalone_publication",
            &publication.id,
            Some(&publication.campaign_id),
            Some(&publication.assignment_id),
            &publication.status,
            external_id.as_deref(),
            &publication,
            &publication.created_at,
        )?;
        self.audit(
            "standalone_publication",
            publication_id,
            "metrics_captured",
            json!({"snapshot_id": snapshot.id}),
        )?;
        Ok(snapshot)
    }
}
