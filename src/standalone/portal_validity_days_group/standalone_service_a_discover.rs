use super::*;

impl<'a> StandaloneService<'a> {
    pub fn discover(&self, mut query: DiscoveryQuery) -> Result<Vec<CreatorMatch>> {
        if query.min_followers.is_some_and(|minimum| minimum < 0) {
            bail!("minimum followers cannot be negative");
        }
        if query.max_rate_minor.is_some_and(|maximum| maximum < 0) {
            bail!("maximum rate cannot be negative");
        }
        if let Some(campaign_id) = &query.campaign_id {
            let campaign: Campaign = self.store.get("campaign", campaign_id)?;
            if query.markets.is_empty() {
                query.markets = campaign.markets;
            }
            if query.languages.is_empty() {
                query.languages = campaign.languages;
            }
            if query.channels.is_empty() {
                query.channels = campaign.channels;
            }
        }
        let creators: Vec<Creator> = self.store.list("creator", None, Some("active"))?;
        let mut matches = Vec::new();
        for creator in creators {
            if creator
                .metadata
                .get("verification_status")
                .and_then(Value::as_str)
                == Some("unverified")
            {
                continue;
            }
            let identities: Vec<CreatorIdentity> =
                self.store
                    .list("creator_identity", Some(&creator.id), None)?;
            let identity_channels: Vec<String> = identities
                .iter()
                .map(|identity| identity.platform.clone())
                .collect();
            if !query.markets.is_empty() && !overlap(&query.markets, &creator.markets) {
                continue;
            }
            if !query.languages.is_empty() && !overlap(&query.languages, &creator.languages) {
                continue;
            }
            if !query.niches.is_empty() && !overlap(&query.niches, &creator.niches) {
                continue;
            }
            if !query.channels.is_empty()
                && !overlap(&query.channels, &identity_channels)
                && !metadata_overlap(&creator.metadata, "channels", &query.channels)
            {
                continue;
            }
            let followers = metadata_i64(&creator.metadata, "followers").unwrap_or_default();
            let rate = metadata_i64(&creator.metadata, "base_rate_minor");
            if query
                .min_followers
                .is_some_and(|minimum| followers < minimum)
            {
                continue;
            }
            if query
                .max_rate_minor
                .is_some_and(|maximum| rate.is_some_and(|candidate| candidate > maximum))
            {
                continue;
            }

            let mut score = BASE_MATCH_SCORE;
            let mut matched = Vec::new();
            let mut missing = Vec::new();
            score_filter(
                &query.markets,
                &creator.markets,
                "market",
                MARKET_WEIGHT,
                &mut score,
                &mut matched,
                &mut missing,
            );
            score_filter(
                &query.languages,
                &creator.languages,
                "language",
                LANGUAGE_WEIGHT,
                &mut score,
                &mut matched,
                &mut missing,
            );
            score_filter(
                &query.niches,
                &creator.niches,
                "niche",
                NICHE_WEIGHT,
                &mut score,
                &mut matched,
                &mut missing,
            );
            score_filter(
                &query.channels,
                &identity_channels,
                "channel",
                CHANNEL_WEIGHT,
                &mut score,
                &mut matched,
                &mut missing,
            );
            let engagement = metadata_f64(&creator.metadata, "engagement_rate").unwrap_or_default();
            if engagement >= STRONG_ENGAGEMENT_RATE {
                score += SIGNAL_BONUS;
                matched.push("engagement".into());
            } else {
                missing.push("engagement evidence".into());
            }
            let completed =
                metadata_i64(&creator.metadata, "completed_campaigns").unwrap_or_default();
            if completed > 0 {
                score += SIGNAL_BONUS;
                matched.push("campaign history".into());
            } else {
                missing.push("campaign history".into());
            }
            let response = metadata_f64(&creator.metadata, "response_rate").unwrap_or_default();
            if response >= RESPONSIVE_RATE {
                score += SIGNAL_BONUS;
                matched.push("response rate".into());
            }
            let portfolio = metadata_i64(&creator.metadata, "portfolio_count").unwrap_or_default();
            if portfolio > 0 {
                score += SIGNAL_BONUS;
                matched.push("portfolio".into());
            } else {
                missing.push("portfolio".into());
            }
            score = score.min(MAX_MATCH_SCORE);
            matches.push(CreatorMatch {
                creator,
                score,
                matched,
                missing,
                signals: json!({
                    "followers": followers,
                    "engagement_rate": engagement,
                    "completed_campaigns": completed,
                    "response_rate": response,
                    "portfolio_count": portfolio,
                    "base_rate_minor": rate,
                    "platforms": identity_channels,
                }),
            });
        }
        matches.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.creator.display_name.cmp(&right.creator.display_name))
        });
        matches.truncate(query.limit.unwrap_or_else(|| DEFAULT_DISCOVERY_LIMIT));
        Ok(matches)
    }
    pub fn launch_campaign(
        &self,
        campaign_id: &str,
        brief_id: &str,
        mut query: DiscoveryQuery,
        offer_minor: Option<i64>,
        shipping_required: bool,
        portal_days: i64,
    ) -> Result<Value> {
        if portal_days <= 0 {
            bail!("portal validity days must be positive");
        }
        let campaign: Campaign = self.store.get("campaign", campaign_id)?;
        let brief: Brief = self.store.get("brief", brief_id)?;
        if !matches!(campaign.status.as_str(), "draft" | "sourcing" | "active") {
            bail!("campaign cannot launch outreach in its current state");
        }
        if let Some(deadline) = campaign.deadline.as_deref() {
            let deadline =
                DateTime::parse_from_rfc3339(deadline).context("invalid campaign deadline")?;
            if deadline <= Utc::now() {
                bail!("campaign deadline has passed");
            }
        }
        if brief.campaign_id != campaign.id || brief.status != "approved" {
            bail!("launch requires an approved brief belonging to the campaign");
        }
        query.campaign_id = Some(campaign.id.clone());
        let matches = self.discover(query)?;
        let existing: Vec<Conversation> =
            self.store.list("conversation", Some(campaign_id), None)?;
        let assignments: Vec<Assignment> =
            self.store.list("assignment", Some(campaign_id), None)?;
        let mut launched = Vec::new();
        let mut skipped = Vec::new();
        let mut reserved: i64 = 0;
        for assignment in assignments
            .iter()
            .filter(|assignment| !matches!(assignment.status.as_str(), "cancelled" | "failed"))
        {
            if let Some(amount) = assignment.compensation_minor {
                let Some(total) = reserved.checked_add(amount) else {
                    bail!("campaign assignment reservation overflow");
                };
                reserved = total;
            }
        }
        for conversation in existing.iter().filter(|conversation| {
            conversation.assignment_id.is_none()
                && !matches!(
                    conversation.status.as_str(),
                    "declined" | "opted_out" | "closed"
                )
        }) {
            if let Some(amount) = conversation.offered_compensation_minor {
                let Some(total) = reserved.checked_add(amount) else {
                    bail!("campaign outreach reservation overflow");
                };
                reserved = total;
            }
        }
        for candidate in matches {
            if existing
                .iter()
                .any(|conversation| conversation.creator_id == candidate.creator.id)
            {
                skipped.push(json!({"creator_id": candidate.creator.id, "reason": "conversation already exists"}));
                continue;
            }
            let Some(compensation) = offer_minor
                .or_else(|| metadata_i64(&candidate.creator.metadata, "base_rate_minor"))
            else {
                skipped.push(json!({"creator_id": candidate.creator.id, "reason": "no offer or base_rate_minor"}));
                continue;
            };
            if compensation <= 0 {
                skipped.push(
                    json!({"creator_id": candidate.creator.id, "reason": "offer must be positive"}),
                );
                continue;
            }
            let Some(proposed_reservation) = reserved.checked_add(compensation) else {
                bail!("campaign outreach reservation overflow");
            };
            if campaign
                .budget_minor
                .is_some_and(|budget| proposed_reservation > budget)
            {
                skipped.push(json!({"creator_id": candidate.creator.id, "reason": "campaign budget exhausted"}));
                continue;
            }
            reserved = proposed_reservation;
            let conversation = self.create_conversation(
                candidate.creator.id.clone(),
                Some(campaign.id.clone()),
                Some(brief.id.clone()),
                Some(compensation),
                campaign.currency.clone(),
                shipping_required,
                None,
            )?;
            let portal = self.create_portal_access(&candidate.creator.id, Some(portal_days))?;
            launched.push(json!({"match_score": candidate.score, "conversation": conversation, "portal": portal}));
        }
        self.audit(
            "campaign",
            campaign_id,
            "standalone_launch",
            json!({"launched": launched.len(), "skipped": skipped.len()}),
        )?;
        Ok(json!({"campaign_id": campaign_id, "launched": launched, "skipped": skipped}))
    }
}
