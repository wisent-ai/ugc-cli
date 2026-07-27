use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    db::Store,
    model::{
        Asset, Assignment, AttributionEvent, Brief, Campaign, Conversation, ConversationMessage,
        Creator, CreatorIdentity, CreatorMatch, DiscoveryQuery, LedgerBalance, LedgerTransfer,
        MetricSnapshot, Payment, PortalAccess, Shipment, StandalonePublication, Submission,
        UsageRights, WorkflowReport,
    },
    service::UgcService,
};

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

    pub fn register_creator(&self, seed: CreatorSeed) -> Result<Value> {
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
        let portal = self.create_portal_access(&creator.id, Some(int("30")))?;
        self.audit("creator", &creator.id, "self_registered", json!({}))?;
        Ok(json!({
            "creator": creator,
            "identities": created_identities,
            "portal": portal,
            "identity_status": "pending_operator_verification",
        }))
    }

    pub fn discover(&self, mut query: DiscoveryQuery) -> Result<Vec<CreatorMatch>> {
        if query.min_followers.is_some_and(|minimum| minimum < zero()) {
            bail!("minimum followers cannot be negative");
        }
        if query.max_rate_minor.is_some_and(|maximum| maximum < zero()) {
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

            let mut score = int("10");
            let mut matched = Vec::new();
            let mut missing = Vec::new();
            score_filter(
                &query.markets,
                &creator.markets,
                "market",
                int("20"),
                &mut score,
                &mut matched,
                &mut missing,
            );
            score_filter(
                &query.languages,
                &creator.languages,
                "language",
                int("20"),
                &mut score,
                &mut matched,
                &mut missing,
            );
            score_filter(
                &query.niches,
                &creator.niches,
                "niche",
                int("25"),
                &mut score,
                &mut matched,
                &mut missing,
            );
            score_filter(
                &query.channels,
                &identity_channels,
                "channel",
                int("10"),
                &mut score,
                &mut matched,
                &mut missing,
            );
            let engagement = metadata_f64(&creator.metadata, "engagement_rate").unwrap_or_default();
            if engagement >= decimal("0.03") {
                score += int("5");
                matched.push("engagement".into());
            } else {
                missing.push("engagement evidence".into());
            }
            let completed =
                metadata_i64(&creator.metadata, "completed_campaigns").unwrap_or_default();
            if completed > zero() {
                score += int("5");
                matched.push("campaign history".into());
            } else {
                missing.push("campaign history".into());
            }
            let response = metadata_f64(&creator.metadata, "response_rate").unwrap_or_default();
            if response >= decimal("0.5") {
                score += int("5");
                matched.push("response rate".into());
            }
            let portfolio = metadata_i64(&creator.metadata, "portfolio_count").unwrap_or_default();
            if portfolio > zero() {
                score += int("5");
                matched.push("portfolio".into());
            } else {
                missing.push("portfolio".into());
            }
            score = score.min(int("100"));
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
        matches.truncate(query.limit.unwrap_or_else(|| usize_from("20")));
        Ok(matches)
    }

    pub fn launch_campaign(
        &self,
        campaign_id: &str,
        brief_id: &str,
        mut query: DiscoveryQuery,
        offer_minor: Option<i64>,
        shipping_required: bool,
    ) -> Result<Value> {
        let campaign: Campaign = self.store.get("campaign", campaign_id)?;
        let brief: Brief = self.store.get("brief", brief_id)?;
        if brief.campaign_id != campaign.id || brief.status != "approved" {
            bail!("launch requires an approved brief belonging to the campaign");
        }
        query.campaign_id = Some(campaign.id.clone());
        let matches = self.discover(query)?;
        let existing: Vec<Conversation> =
            self.store.list("conversation", Some(campaign_id), None)?;
        let mut launched = Vec::new();
        let mut skipped = Vec::new();
        let mut reserved = zero();
        for conversation in existing.iter().filter(|conversation| {
            !matches!(
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
            if compensation <= zero() {
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
            let portal = self.create_portal_access(&candidate.creator.id, Some(int("30")))?;
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

    pub fn create_conversation(
        &self,
        creator_id: String,
        campaign_id: Option<String>,
        brief_id: Option<String>,
        offered_compensation_minor: Option<i64>,
        currency: String,
        shipping_required: bool,
        initial_message: Option<String>,
    ) -> Result<Value> {
        let creator: Creator = self.store.get("creator", &creator_id)?;
        let campaign = campaign_id
            .as_deref()
            .map(|campaign| self.store.get::<Campaign>("campaign", campaign))
            .transpose()?;
        if let Some(brief) = &brief_id {
            let brief_record: Brief = self.store.get("brief", brief)?;
            if campaign_id.as_deref() != Some(&brief_record.campaign_id) {
                bail!("brief does not belong to conversation campaign");
            }
            if brief_record.status != "approved" {
                bail!("conversation brief must be approved");
            }
        }
        if currency.trim().is_empty() {
            bail!("conversation currency is required");
        }
        if campaign
            .as_ref()
            .is_some_and(|campaign| !currency.eq_ignore_ascii_case(&campaign.currency))
        {
            bail!("conversation currency must match campaign currency");
        }
        if offered_compensation_minor.is_some_and(|amount| amount <= zero()) {
            bail!("offered compensation must be positive");
        }
        let now = Store::now();
        let conversation = Conversation {
            id: Store::id(),
            creator_id: creator_id.clone(),
            campaign_id: campaign_id.clone(),
            brief_id,
            assignment_id: None,
            status: "open".into(),
            stage: "outreach".into(),
            offered_compensation_minor,
            currency,
            shipping_required,
            next_action_at: None,
            last_inbound_at: None,
            last_outbound_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        self.save_conversation(&conversation)?;
        let body = initial_message
            .unwrap_or_else(|| self.outreach_template(&creator, campaign_id.as_deref()));
        let message = self.add_conversation_message(
            &conversation.id,
            "outbound",
            "local_portal",
            body,
            true,
            None,
        )?;
        self.audit(
            "conversation",
            &conversation.id,
            "created",
            json!({"creator_id": creator_id}),
        )?;
        Ok(json!({"conversation": conversation, "message": message}))
    }

    pub fn receive_message(
        &self,
        conversation_id: &str,
        body: String,
        channel: String,
        external_id: Option<String>,
    ) -> Result<Value> {
        let mut conversation: Conversation = self.store.get("conversation", conversation_id)?;
        if matches!(
            conversation.status.as_str(),
            "closed" | "declined" | "opted_out"
        ) {
            bail!("conversation is not open");
        }
        let intent = classify_intent(&body);
        let inbound = self.add_conversation_message(
            conversation_id,
            "inbound",
            &channel,
            body,
            false,
            external_id,
        )?;
        conversation.last_inbound_at = Some(inbound.created_at.clone());
        conversation.updated_at = Store::now();
        conversation.next_action_at = if matches!(intent.as_str(), "pricing" | "question" | "other")
        {
            Some(Store::now())
        } else {
            None
        };
        match intent.as_str() {
            "opt_out" => {
                conversation.status = "opted_out".into();
                conversation.stage = "closed".into();
            }
            "declined" => {
                conversation.status = "declined".into();
                conversation.stage = "closed".into();
            }
            "interested" => {
                conversation.status = "interested".into();
                conversation.stage = "qualification".into();
            }
            "accepted" => {
                conversation.status = "accepted".into();
                conversation.stage = "contracting".into();
            }
            "pricing" => {
                conversation.stage = "negotiation".into();
            }
            "submitted" => {
                conversation.stage = "delivery".into();
            }
            _ => {}
        }
        self.save_conversation(&conversation)?;
        let assignment = if intent == "accepted" {
            let assignment = self.accept_conversation(conversation_id)?;
            conversation = self.store.get("conversation", conversation_id)?;
            Some(assignment)
        } else {
            None
        };
        let automated_reply = self.automatic_reply(&conversation, &intent)?;
        self.audit(
            "conversation",
            conversation_id,
            "inbound_received",
            json!({"intent": intent, "automated_reply": automated_reply.is_some()}),
        )?;
        Ok(
            json!({"conversation": conversation, "inbound": inbound, "automated_reply": automated_reply, "assignment": assignment}),
        )
    }

    pub fn send_message(
        &self,
        conversation_id: &str,
        body: String,
        channel: String,
        automated: bool,
    ) -> Result<ConversationMessage> {
        let conversation: Conversation = self.store.get("conversation", conversation_id)?;
        if matches!(
            conversation.status.as_str(),
            "closed" | "declined" | "opted_out"
        ) {
            bail!("conversation is not open");
        }
        self.add_conversation_message(conversation_id, "outbound", &channel, body, automated, None)
    }

    pub fn list_conversations(
        &self,
        campaign_id: Option<&str>,
        creator_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<Conversation>> {
        let conversations: Vec<Conversation> =
            self.store.list("conversation", campaign_id, status)?;
        Ok(match creator_id {
            Some(creator) => conversations
                .into_iter()
                .filter(|conversation| conversation.creator_id == creator)
                .collect(),
            None => conversations,
        })
    }

    pub fn messages(&self, conversation_id: &str) -> Result<Vec<ConversationMessage>> {
        self.store
            .list("conversation_message", Some(conversation_id), None)
    }

    pub fn accept_conversation(&self, conversation_id: &str) -> Result<Assignment> {
        let mut conversation: Conversation = self.store.get("conversation", conversation_id)?;
        if let Some(assignment_id) = &conversation.assignment_id {
            return self.store.get("assignment", assignment_id);
        }
        if conversation.status == "opted_out" || conversation.status == "declined" {
            bail!("creator declined this conversation");
        }
        let campaign_id = conversation
            .campaign_id
            .clone()
            .context("conversation has no campaign")?;
        let brief_id = conversation
            .brief_id
            .clone()
            .context("conversation has no brief")?;
        let creator: Creator = self.store.get("creator", &conversation.creator_id)?;
        let compensation = conversation
            .offered_compensation_minor
            .or_else(|| metadata_i64(&creator.metadata, "base_rate_minor"))
            .context("conversation has no compensation offer")?;
        if compensation <= zero() {
            bail!("compensation must be positive");
        }
        let brief: Brief = self.store.get("brief", &brief_id)?;
        let service = self.core();
        let assignment = service.create_assignment(
            campaign_id,
            brief_id,
            conversation.creator_id.clone(),
            None,
            Some(compensation),
            conversation.currency.clone(),
            "echo".into(),
            None,
            conversation.shipping_required,
            brief.revision_limit,
            Some(format!("local-conversation:{conversation_id}")),
        )?;
        let assignment = service.assignment_status(&assignment.id, "accepted")?;
        conversation.assignment_id = Some(assignment.id.clone());
        conversation.status = "accepted".into();
        conversation.stage = "contracted".into();
        conversation.updated_at = Store::now();
        self.save_conversation(&conversation)?;
        self.audit(
            "conversation",
            conversation_id,
            "accepted",
            json!({"assignment_id": assignment.id}),
        )?;
        Ok(assignment)
    }

    pub fn create_portal_access(&self, creator_id: &str, days: Option<i64>) -> Result<Value> {
        let _: Creator = self.store.get("creator", creator_id)?;
        if days.is_some_and(|days| days <= zero()) {
            bail!("portal validity days must be positive");
        }
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let token_hash = hash_token(&token);
        let now = Store::now();
        let expires_at = days.map(|days| (Utc::now() + Duration::days(days)).to_rfc3339());
        let access = PortalAccess {
            id: Store::id(),
            creator_id: creator_id.into(),
            token_hash: token_hash.clone(),
            status: "active".into(),
            expires_at,
            created_at: now.clone(),
            last_used_at: None,
        };
        self.store.put(
            "portal_access",
            &access.id,
            Some(creator_id),
            None,
            &access.status,
            Some(&token_hash),
            &access,
            &now,
        )?;
        self.audit(
            "portal_access",
            &access.id,
            "created",
            json!({"creator_id": creator_id}),
        )?;
        Ok(json!({"access": access, "token": token}))
    }

    pub fn resolve_portal(&self, token: &str) -> Result<PortalAccess> {
        let hash = hash_token(token);
        let mut access: PortalAccess = self
            .store
            .find_external("portal_access", &hash)?
            .context("invalid portal token")?;
        if access.status != "active" {
            bail!("portal access is not active");
        }
        if let Some(expires_at) = &access.expires_at {
            let expiry = DateTime::parse_from_rfc3339(expires_at)?;
            if expiry < Utc::now() {
                bail!("portal access has expired");
            }
        }
        access.last_used_at = Some(Store::now());
        self.store.put(
            "portal_access",
            &access.id,
            Some(&access.creator_id),
            None,
            &access.status,
            Some(&access.token_hash),
            &access,
            &access.created_at,
        )?;
        Ok(access)
    }

    pub fn revoke_portal(&self, access_id: &str) -> Result<PortalAccess> {
        let mut access: PortalAccess = self.store.get("portal_access", access_id)?;
        access.status = "revoked".into();
        self.store.put(
            "portal_access",
            &access.id,
            Some(&access.creator_id),
            None,
            &access.status,
            Some(&access.token_hash),
            &access,
            &access.created_at,
        )?;
        self.audit("portal_access", access_id, "revoked", json!({}))?;
        Ok(access)
    }

    pub fn fund_escrow(
        &self,
        assignment_id: &str,
        amount_minor: i64,
        currency: String,
        idempotency_key: String,
    ) -> Result<LedgerTransfer> {
        self.transfer(
            "external:funding".into(),
            format!("escrow:{assignment_id}"),
            amount_minor,
            currency,
            "escrow_funding".into(),
            Some(assignment_id.into()),
            None,
            None,
            idempotency_key,
            None,
        )
    }

    pub fn release_payment(&self, payment_id: &str, idempotency_key: String) -> Result<Value> {
        let payment: Payment = self.store.get("payment", payment_id)?;
        if payment.owner != "echo" {
            bail!("only Echo-owned payments use the standalone ledger");
        }
        if payment.status == "paid" {
            return Ok(json!({"payment": payment, "already_paid": true}));
        }
        if payment.status == "processing" {
            let transfers: Vec<LedgerTransfer> =
                self.store
                    .list("ledger_transfer", Some(&payment.assignment_id), None)?;
            let transfer = transfers.into_iter().find(|transfer| {
                transfer.payment_id.as_deref() == Some(payment_id)
                    && transfer.kind == "creator_release"
            });
            return Ok(json!({"payment": payment, "already_released": true, "transfer": transfer}));
        }
        if payment.status != "pending" {
            bail!("payment must be pending before release");
        }
        let assignment: Assignment = self.store.get("assignment", &payment.assignment_id)?;
        let submission_id = payment
            .submission_id
            .as_deref()
            .context("payment has no approved submission")?;
        let submission: Submission = self.store.get("submission", submission_id)?;
        if submission.status != "approved" {
            bail!("submission must be approved before release");
        }
        let assets: Vec<Asset> = self.store.list("asset", Some(&submission.id), None)?;
        let rights: Vec<UsageRights> =
            self.store
                .list("usage_rights", Some(&assignment.id), None)?;
        let applicable_rights: Vec<&UsageRights> = rights
            .iter()
            .filter(|rights| {
                rights.asset_id.is_none()
                    || assets
                        .iter()
                        .any(|asset| rights.asset_id.as_deref() == Some(&asset.id))
            })
            .collect();
        if applicable_rights.is_empty() {
            bail!("usage rights for the approved submission must be recorded before release");
        }
        if applicable_rights
            .iter()
            .any(|rights| !rights.model_release || !rights.music_cleared)
        {
            bail!("applicable rights must confirm model release and music clearance");
        }
        let escrow = format!("escrow:{}", assignment.id);
        let balance = self.balance(&escrow, &payment.currency)?.balance_minor;
        if balance < payment.amount_minor {
            bail!(
                "escrow balance {balance} is below payment amount {}",
                payment.amount_minor
            );
        }
        let transfer = self.transfer(
            escrow,
            format!("creator:{}", assignment.creator_id),
            payment.amount_minor,
            payment.currency.clone(),
            "creator_release".into(),
            Some(assignment.id.clone()),
            Some(payment.id.clone()),
            None,
            idempotency_key,
            None,
        )?;
        let payment = self
            .core()
            .payment_status(payment_id, "processing", None, None)?;
        Ok(json!({"payment": payment, "transfer": transfer}))
    }

    pub fn settle_offline(
        &self,
        payment_id: &str,
        reference: String,
        idempotency_key: String,
    ) -> Result<Value> {
        if reference.trim().is_empty() {
            bail!("offline settlement reference is required");
        }
        let payment: Payment = self.store.get("payment", payment_id)?;
        if payment.status == "paid" {
            return Ok(json!({"payment": payment, "already_paid": true}));
        }
        if payment.status != "processing" {
            bail!("payment must be released to creator balance before settlement");
        }
        let assignment: Assignment = self.store.get("assignment", &payment.assignment_id)?;
        let account = format!("creator:{}", assignment.creator_id);
        let balance = self.balance(&account, &payment.currency)?.balance_minor;
        if balance < payment.amount_minor {
            bail!("creator ledger balance is below payment amount");
        }
        let transfer = self.transfer(
            account,
            "external:offline_payout".into(),
            payment.amount_minor,
            payment.currency.clone(),
            "offline_settlement".into(),
            Some(assignment.id.clone()),
            Some(payment.id.clone()),
            Some(reference.clone()),
            idempotency_key,
            None,
        )?;
        let payment = self
            .core()
            .payment_status(payment_id, "paid", Some(reference), None)?;
        Ok(json!({"payment": payment, "transfer": transfer}))
    }

    pub fn reverse_transfer(
        &self,
        transfer_id: &str,
        reason: String,
        idempotency_key: String,
    ) -> Result<LedgerTransfer> {
        if reason.trim().is_empty() {
            bail!("reversal reason is required");
        }
        let original: LedgerTransfer = self.store.get("ledger_transfer", transfer_id)?;
        if original.status != "posted" {
            bail!("only posted transfers can be reversed");
        }
        let existing: Vec<LedgerTransfer> = self.store.list("ledger_transfer", None, None)?;
        if existing
            .iter()
            .any(|transfer| transfer.reversal_of.as_deref() == Some(transfer_id))
        {
            bail!("transfer already has a reversal");
        }
        self.transfer(
            original.to_account,
            original.from_account,
            original.amount_minor,
            original.currency,
            "reversal".into(),
            original.assignment_id,
            original.payment_id,
            Some(reason),
            idempotency_key,
            Some(transfer_id.into()),
        )
    }

    pub fn balance(&self, account: &str, currency: &str) -> Result<LedgerBalance> {
        let transfers: Vec<LedgerTransfer> =
            self.store.list("ledger_transfer", None, Some("posted"))?;
        let mut balance = zero();
        for transfer in transfers
            .iter()
            .filter(|transfer| transfer.currency.eq_ignore_ascii_case(currency))
        {
            if transfer.to_account == account {
                balance += transfer.amount_minor;
            }
            if transfer.from_account == account {
                balance -= transfer.amount_minor;
            }
        }
        Ok(LedgerBalance {
            account: account.into(),
            currency: currency.into(),
            balance_minor: balance,
        })
    }

    pub fn ledger(&self, assignment_id: Option<&str>) -> Result<Vec<LedgerTransfer>> {
        let transfers: Vec<LedgerTransfer> =
            self.store.list("ledger_transfer", assignment_id, None)?;
        Ok(transfers)
    }

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
        if counters.iter().any(|value| *value < zero()) {
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
        if value_minor.is_some_and(|value| value < zero()) {
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
        let mut attributed_revenue_minor = zero();
        let mut attributed_conversions = zero();
        let mut attributed_events = zero();
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
                    attributed_conversions += int("1");
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
        let canonical_conversions = if attributed_conversions > zero() {
            attributed_conversions
        } else {
            totals.conversions
        };
        let canonical_revenue_minor = if attributed_revenue_minor > zero() {
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

    pub fn advance_workflow(
        &self,
        campaign_id: &str,
        apply: bool,
        release_payments: bool,
    ) -> Result<WorkflowReport> {
        let service = self.core();
        let mut campaign: Campaign = self.store.get("campaign", campaign_id)?;
        let briefs: Vec<Brief> = self.store.list("brief", Some(campaign_id), None)?;
        let conversations: Vec<Conversation> =
            self.store.list("conversation", Some(campaign_id), None)?;
        let mut assignments: Vec<Assignment> =
            self.store.list("assignment", Some(campaign_id), None)?;
        let publications: Vec<StandalonePublication> =
            self.store
                .list("standalone_publication", Some(campaign_id), None)?;
        let approved_brief = briefs.iter().any(|brief| brief.status == "approved");
        let mut actions = Vec::new();
        let mut blockers = Vec::new();

        if campaign.status == "draft" {
            if approved_brief {
                actions.push("campaign draft -> ready".into());
                if apply {
                    campaign = service.campaign_status(campaign_id, "ready")?;
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
                    campaign = service.campaign_status(campaign_id, "published")?;
                }
            }
        }
        if campaign.status == "published" {
            actions.push("campaign published -> sourcing".into());
            if apply {
                campaign = service.campaign_status(campaign_id, "sourcing")?;
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
                    campaign = service.campaign_status(campaign_id, "active")?;
                }
            } else {
                blockers.push("obtain at least one accepted creator assignment".into());
            }
        }

        for assignment in &mut assignments {
            let shipments: Vec<Shipment> =
                self.store.list("shipment", Some(&assignment.id), None)?;
            if assignment.status == "accepted" {
                if assignment.shipping_required {
                    if shipments.iter().any(|shipment| {
                        matches!(
                            shipment.status.as_str(),
                            "ready_to_ship" | "shipped" | "delivered"
                        )
                    }) {
                        actions.push(format!(
                            "assignment {} accepted -> product_shipping",
                            assignment.id
                        ));
                        if apply {
                            *assignment =
                                service.assignment_status(&assignment.id, "product_shipping")?;
                        }
                    } else {
                        blockers.push(format!(
                            "collect shipping address for assignment {}",
                            assignment.id
                        ));
                    }
                } else {
                    actions.push(format!(
                        "assignment {} accepted -> in_production",
                        assignment.id
                    ));
                    if apply {
                        *assignment = service.assignment_status(&assignment.id, "in_production")?;
                    }
                }
            }
            if assignment.status == "product_shipping" {
                if shipments
                    .iter()
                    .any(|shipment| shipment.status == "delivered")
                {
                    actions.push(format!(
                        "assignment {} product_shipping -> in_production",
                        assignment.id
                    ));
                    if apply {
                        *assignment = service.assignment_status(&assignment.id, "in_production")?;
                    }
                } else {
                    blockers.push(format!(
                        "deliver product shipment for assignment {}",
                        assignment.id
                    ));
                }
            }
            let submissions: Vec<Submission> =
                self.store.list("submission", Some(&assignment.id), None)?;
            let approved_submission = submissions
                .iter()
                .find(|submission| submission.status == "approved");
            if assignment.status == "in_production"
                && submissions
                    .iter()
                    .all(|submission| submission.status == "rejected")
            {
                blockers.push(format!("submit media for assignment {}", assignment.id));
            }
            if submissions
                .iter()
                .any(|submission| submission.status == "received")
            {
                blockers.push(format!("run QC for assignment {}", assignment.id));
            }
            if submissions
                .iter()
                .any(|submission| submission.status == "pending_review")
            {
                blockers.push(format!(
                    "review submission for assignment {}",
                    assignment.id
                ));
            }
            if assignment.status == "revision_requested" {
                blockers.push(format!(
                    "submit a revised asset for assignment {}",
                    assignment.id
                ));
            }
            if assignment.status == "submitted" && approved_submission.is_some() {
                actions.push(format!(
                    "assignment {} submitted -> approved",
                    assignment.id
                ));
                if apply {
                    *assignment = service.assignment_status(&assignment.id, "approved")?;
                }
            }
            let rights: Vec<UsageRights> =
                self.store
                    .list("usage_rights", Some(&assignment.id), None)?;
            let approved_assets: Vec<Asset> = match approved_submission {
                Some(submission) => self.store.list("asset", Some(&submission.id), None)?,
                None => Vec::new(),
            };
            let applicable_rights: Vec<&UsageRights> = rights
                .iter()
                .filter(|rights| {
                    rights.asset_id.is_none()
                        || approved_assets
                            .iter()
                            .any(|asset| rights.asset_id.as_deref() == Some(&asset.id))
                })
                .collect();
            let rights_ready = !applicable_rights.is_empty()
                && applicable_rights
                    .iter()
                    .all(|rights| rights.model_release && rights.music_cleared);
            if assignment.status == "approved" && rights_ready {
                actions.push(format!("assignment {} approved -> licensed", assignment.id));
                if apply {
                    *assignment = service.assignment_status(&assignment.id, "licensed")?;
                }
            } else if assignment.status == "approved" {
                blockers.push(format!(
                    "record complete rights for assignment {} approved submission",
                    assignment.id
                ));
            }
            let mut payments: Vec<Payment> =
                self.store.list("payment", Some(&assignment.id), None)?;
            if assignment.status == "licensed" && payments.is_empty() {
                if let (Some(submission), Some(amount)) =
                    (approved_submission, assignment.compensation_minor)
                {
                    actions.push(format!("create payment for assignment {}", assignment.id));
                    if apply {
                        let payment = service.create_payment(
                            assignment.id.clone(),
                            Some(submission.id.clone()),
                            amount,
                            assignment.currency.clone(),
                            None,
                        )?;
                        payments.push(payment);
                    }
                } else {
                    blockers.push(format!(
                        "assignment {} lacks approved submission or compensation",
                        assignment.id
                    ));
                }
            }
            if assignment.status == "licensed" && release_payments {
                for payment in payments
                    .iter()
                    .filter(|payment| payment.status == "pending")
                {
                    let escrow = format!("escrow:{}", assignment.id);
                    let balance = self.balance(&escrow, &payment.currency)?.balance_minor;
                    if balance >= payment.amount_minor {
                        actions.push(format!("release payment {} from escrow", payment.id));
                        if apply {
                            self.release_payment(
                                &payment.id,
                                format!("workflow-release:{}", payment.id),
                            )?;
                        }
                    } else {
                        blockers.push(format!("fund escrow for payment {}", payment.id));
                    }
                }
                if apply {
                    payments = self.store.list("payment", Some(&assignment.id), None)?;
                }
            }
            if assignment.status == "licensed"
                && payments.iter().any(|payment| payment.status == "paid")
            {
                actions.push(format!("assignment {} licensed -> paid", assignment.id));
                if apply {
                    *assignment = service.assignment_status(&assignment.id, "paid")?;
                }
            } else if assignment.status == "licensed" {
                if payments
                    .iter()
                    .any(|payment| payment.status == "processing")
                {
                    blockers.push(format!(
                        "record offline settlement for assignment {}",
                        assignment.id
                    ));
                } else {
                    blockers.push(format!(
                        "fund and release payment for assignment {}",
                        assignment.id
                    ));
                }
            }
            if assignment.status == "paid" {
                actions.push(format!("assignment {} paid -> completed", assignment.id));
                if apply {
                    *assignment = service.assignment_status(&assignment.id, "completed")?;
                }
            }
        }

        if campaign.status == "active"
            && !assignments.is_empty()
            && assignments
                .iter()
                .all(|assignment| matches!(assignment.status.as_str(), "completed" | "cancelled"))
        {
            if publications.is_empty() {
                blockers.push("record at least one publication before campaign completion".into());
            } else {
                actions.push("campaign active -> completed".into());
                if apply {
                    campaign = service.campaign_status(campaign_id, "completed")?;
                }
            }
        }
        if conversations.is_empty() {
            blockers.push("no creator conversations".into());
        }
        if assignments.is_empty() {
            blockers.push("no creator assignments".into());
        }
        let report = WorkflowReport {
            campaign_id: campaign_id.into(),
            status: campaign.status,
            blockers: unique(blockers),
            actions: unique(actions),
            counts: json!({
                "briefs": briefs.len(), "conversations": conversations.len(),
                "assignments": assignments.len(), "publications": publications.len()
            }),
            updated_at: Store::now(),
        };
        self.audit(
            "campaign",
            campaign_id,
            if apply {
                "workflow_advanced"
            } else {
                "workflow_inspected"
            },
            serde_json::to_value(&report)?,
        )?;
        Ok(report)
    }

    pub fn dashboard(&self) -> Result<Value> {
        let campaigns: Vec<Campaign> = self.store.list("campaign", None, None)?;
        let creators: Vec<Creator> = self.store.list("creator", None, None)?;
        let conversations: Vec<Conversation> = self.store.list("conversation", None, None)?;
        let assignments: Vec<Assignment> = self.store.list("assignment", None, None)?;
        let submissions: Vec<Submission> = self.store.list("submission", None, None)?;
        let payments: Vec<Payment> = self.store.list("payment", None, None)?;
        let publications: Vec<StandalonePublication> =
            self.store.list("standalone_publication", None, None)?;
        let attention = json!({
            "unanswered_conversations": conversations.iter().filter(|item| {
                item.last_inbound_at > item.last_outbound_at
                    && !matches!(item.status.as_str(), "closed" | "declined" | "opted_out")
            }).count(),
            "operator_follow_ups": conversations.iter().filter(|item| {
                item.next_action_at.is_some()
                    && !matches!(item.status.as_str(), "closed" | "declined" | "opted_out")
            }).count(),
            "pending_reviews": submissions.iter().filter(|item| item.status == "pending_review").count(),
            "payments_to_release": payments.iter().filter(|item| item.status == "pending").count(),
            "payments_to_settle": payments.iter().filter(|item| item.status == "processing").count(),
            "active_publications_without_metrics": publications.iter().filter(|item| item.status == "active" && item.last_checked_at.is_none()).count(),
        });
        Ok(json!({
            "counts": {
                "campaigns": campaigns.len(), "creators": creators.len(), "conversations": conversations.len(),
                "assignments": assignments.len(), "submissions": submissions.len(), "payments": payments.len(),
                "publications": publications.len()
            },
            "attention": attention,
            "generated_at": Store::now(),
        }))
    }

    fn automatic_reply(
        &self,
        conversation: &Conversation,
        intent: &str,
    ) -> Result<Option<ConversationMessage>> {
        let body = match intent {
            "interested" => Some(format!("Thanks for your interest. The current offer is {} {} in minor units. Reply ACCEPT to confirm, or send your requested rate and questions.", conversation.offered_compensation_minor.unwrap_or_default(), conversation.currency)),
            "pricing" => Some(format!("Thanks. Our recorded offer is {} {} in minor units. Send the rate you can accept and any scope assumptions; an operator will review changes.", conversation.offered_compensation_minor.unwrap_or_default(), conversation.currency)),
            "question" => Some("Thanks for the question. We recorded it for the campaign operator. You can continue this thread; requirements and decisions remain attached to your portal record.".into()),
            "accepted" => Some("Acceptance recorded. Your assignment is now available in this portal with the agreed compensation and approved brief.".into()),
            "submitted" => Some("Delivery notice received. Upload or register the submission in the portal so it can enter QC and human review.".into()),
            "opt_out" | "declined" => None,
            _ => Some("Thanks—we recorded your message. A campaign operator can review this thread and respond here.".into()),
        };
        body.map(|body| {
            self.add_conversation_message(
                &conversation.id,
                "outbound",
                "local_portal",
                body,
                true,
                None,
            )
        })
        .transpose()
    }

    fn add_conversation_message(
        &self,
        conversation_id: &str,
        direction: &str,
        channel: &str,
        body: String,
        automated: bool,
        external_message_id: Option<String>,
    ) -> Result<ConversationMessage> {
        if body.trim().is_empty() {
            bail!("message body is required");
        }
        let mut conversation: Conversation = self.store.get("conversation", conversation_id)?;
        let intent = (direction == "inbound").then(|| classify_intent(&body));
        let message = ConversationMessage {
            id: Store::id(),
            conversation_id: conversation_id.into(),
            creator_id: conversation.creator_id.clone(),
            direction: direction.into(),
            channel: channel.into(),
            body,
            intent,
            automated,
            external_message_id: external_message_id.clone(),
            created_at: Store::now(),
        };
        self.store.put(
            "conversation_message",
            &message.id,
            Some(conversation_id),
            Some(&conversation.creator_id),
            "delivered",
            external_message_id.as_deref(),
            &message,
            &message.created_at,
        )?;
        if direction == "outbound" {
            conversation.last_outbound_at = Some(message.created_at.clone());
            if !automated {
                conversation.next_action_at = None;
            }
        }
        if direction == "inbound" {
            conversation.last_inbound_at = Some(message.created_at.clone());
        }
        conversation.updated_at = Store::now();
        self.save_conversation(&conversation)?;
        self.audit(
            "conversation",
            conversation_id,
            "message_recorded",
            json!({"message_id": message.id, "direction": direction, "automated": automated}),
        )?;
        Ok(message)
    }

    fn transfer(
        &self,
        from_account: String,
        to_account: String,
        amount_minor: i64,
        currency: String,
        kind: String,
        assignment_id: Option<String>,
        payment_id: Option<String>,
        reference: Option<String>,
        idempotency_key: String,
        reversal_of: Option<String>,
    ) -> Result<LedgerTransfer> {
        if amount_minor <= zero() {
            bail!("ledger amount must be positive");
        }
        if from_account == to_account {
            bail!("ledger accounts must differ");
        }
        if currency.trim().is_empty() || idempotency_key.trim().is_empty() {
            bail!("ledger currency and idempotency key are required");
        }
        if let Some(existing) = self
            .store
            .find_external::<LedgerTransfer>("ledger_transfer", &idempotency_key)?
        {
            let same_request = existing.from_account == from_account
                && existing.to_account == to_account
                && existing.amount_minor == amount_minor
                && existing.currency.eq_ignore_ascii_case(&currency)
                && existing.kind == kind
                && existing.assignment_id == assignment_id
                && existing.payment_id == payment_id
                && existing.reference == reference
                && existing.reversal_of == reversal_of;
            if same_request {
                return Ok(existing);
            }
            bail!("ledger idempotency key was already used for a different transfer");
        }
        if let Some(assignment_id) = &assignment_id {
            let assignment: Assignment = self.store.get("assignment", assignment_id)?;
            if !assignment.currency.eq_ignore_ascii_case(&currency) {
                bail!("ledger currency must match assignment currency");
            }
        }
        if let Some(payment_id) = &payment_id {
            let payment: Payment = self.store.get("payment", payment_id)?;
            if assignment_id.as_deref() != Some(&payment.assignment_id) {
                bail!("ledger payment does not belong to assignment");
            }
            if !payment.currency.eq_ignore_ascii_case(&currency) {
                bail!("ledger currency must match payment currency");
            }
            if payment.amount_minor != amount_minor {
                bail!("ledger amount must match payment amount");
            }
        }
        let now = Store::now();
        let transfer = LedgerTransfer {
            id: Store::id(),
            from_account,
            to_account,
            amount_minor,
            currency,
            kind,
            status: "posted".into(),
            assignment_id: assignment_id.clone(),
            payment_id,
            reference,
            idempotency_key: idempotency_key.clone(),
            reversal_of,
            created_at: now.clone(),
            posted_at: now.clone(),
        };
        self.store.put(
            "ledger_transfer",
            &transfer.id,
            assignment_id.as_deref(),
            None,
            &transfer.status,
            Some(&idempotency_key),
            &transfer,
            &now,
        )?;
        self.audit("ledger_transfer", &transfer.id, "posted", json!({"from": transfer.from_account, "to": transfer.to_account, "amount_minor": amount_minor, "currency": transfer.currency}))?;
        Ok(transfer)
    }

    fn outreach_template(&self, creator: &Creator, campaign_id: Option<&str>) -> String {
        match campaign_id.and_then(|id| self.store.get::<Campaign>("campaign", id).ok()) {
            Some(campaign) => format!(
                "Hi {}, we would like to invite you to the '{}' UGC campaign for {}. Reply INTERESTED to continue, ask any questions here, or reply STOP to opt out.",
                creator.display_name, campaign.name, campaign.product
            ),
            None => format!(
                "Hi {}, we would like to discuss a UGC collaboration. Reply INTERESTED to continue, ask any questions here, or reply STOP to opt out.",
                creator.display_name
            ),
        }
    }

    fn save_conversation(&self, conversation: &Conversation) -> Result<()> {
        self.store.put(
            "conversation",
            &conversation.id,
            conversation.campaign_id.as_deref(),
            Some(&conversation.creator_id),
            &conversation.status,
            None,
            conversation,
            &conversation.created_at,
        )
    }

    fn core(&self) -> UgcService<'_> {
        UgcService {
            store: self.store,
            actor: self.actor,
        }
    }

    fn audit(&self, kind: &str, id: &str, action: &str, details: Value) -> Result<()> {
        self.store.audit(kind, id, action, self.actor, &details)
    }
}

#[derive(Default)]
struct MetricTotals {
    views: i64,
    likes: i64,
    comments: i64,
    shares: i64,
    saves: i64,
    clicks: i64,
    conversions: i64,
    revenue_minor: i64,
    spend_minor: i64,
}

impl MetricTotals {
    fn add(&mut self, value: &MetricSnapshot) {
        self.views += value.views;
        self.likes += value.likes;
        self.comments += value.comments;
        self.shares += value.shares;
        self.saves += value.saves;
        self.clicks += value.clicks;
        self.conversions += value.conversions;
        self.revenue_minor += value.revenue_minor;
        self.spend_minor += value.spend_minor;
    }
}

fn classify_intent(body: &str) -> String {
    let text = body.to_lowercase();
    let contains = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if contains(&["unsubscribe", "stop", "remove me", "nie pisz", "wypisz"]) {
        "opt_out"
    } else if contains(&[
        "not interested",
        "no thanks",
        "decline",
        "odmawiam",
        "nie jestem zainteres",
    ]) {
        "declined"
    } else if contains(&[
        "accept",
        "i agree",
        "confirmed",
        "akceptuję",
        "akceptuje",
        "zgadzam się",
    ]) {
        "accepted"
    } else if contains(&[
        "interested",
        "sounds good",
        "zainteresowan",
        "chętnie",
        "chetnie",
    ]) {
        "interested"
    } else if contains(&[
        "price",
        "rate",
        "budget",
        "fee",
        "stawka",
        "cena",
        "wynagrodzenie",
    ]) {
        "pricing"
    } else if contains(&[
        "submitted",
        "uploaded",
        "delivered",
        "wysł",
        "gotowe",
        "przesł",
    ]) {
        "submitted"
    } else if text.contains('?') {
        "question"
    } else {
        "other"
    }
    .into()
}

fn score_filter(
    expected: &[String],
    actual: &[String],
    label: &str,
    weight: i64,
    score: &mut i64,
    matched: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    if expected.is_empty() {
        return;
    }
    if overlap(expected, actual) {
        *score += weight;
        matched.push(label.into());
    } else {
        missing.push(label.into());
    }
}

fn overlap(left: &[String], right: &[String]) -> bool {
    left.iter()
        .any(|left| right.iter().any(|right| left.eq_ignore_ascii_case(right)))
}

fn metadata_overlap(metadata: &Value, key: &str, expected: &[String]) -> bool {
    metadata
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values.iter().filter_map(Value::as_str).any(|value| {
                expected
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(value))
            })
        })
}

fn metadata_i64(metadata: &Value, key: &str) -> Option<i64> {
    metadata
        .get(key)
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
}

fn metadata_f64(metadata: &Value, key: &str) -> Option<f64> {
    metadata
        .get(key)
        .and_then(|value| value.as_f64().or_else(|| value.as_str()?.parse().ok()))
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn ratio(numerator: i64, denominator: i64) -> Option<f64> {
    (denominator > zero()).then(|| numerator as f64 / denominator as f64)
}

fn is_conversion_event(event_type: &str) -> bool {
    matches!(
        event_type.to_ascii_lowercase().as_str(),
        "conversion" | "order" | "purchase" | "sale"
    )
}

fn unique(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn zero() -> i64 {
    "".len() as i64
}
fn int(value: &str) -> i64 {
    value.parse().expect("valid internal integer")
}
fn decimal(value: &str) -> f64 {
    value.parse().expect("valid internal decimal")
}
fn usize_from(value: &str) -> usize {
    value.parse().expect("valid internal usize")
}
