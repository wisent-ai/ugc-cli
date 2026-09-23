use super::*;

impl<'a> StandaloneService<'a> {
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
    pub(crate) fn automatic_reply(
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
    pub(crate) fn add_conversation_message(
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
    pub(crate) fn transfer(
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
        if amount_minor <= 0 {
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
    pub(crate) fn outreach_template(&self, creator: &Creator, campaign_id: Option<&str>) -> String {
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
    pub(crate) fn save_conversation(&self, conversation: &Conversation) -> Result<()> {
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
    pub(crate) fn core(&self) -> UgcService<'_> {
        UgcService {
            store: self.store,
            actor: self.actor,
        }
    }
    pub(crate) fn audit(&self, kind: &str, id: &str, action: &str, details: Value) -> Result<()> {
        self.store.audit(kind, id, action, self.actor, &details)
    }
}

#[derive(Default)]
pub(crate) struct MetricTotals {
    pub(crate) views: i64,
    pub(crate) likes: i64,
    pub(crate) comments: i64,
    pub(crate) shares: i64,
    pub(crate) saves: i64,
    pub(crate) clicks: i64,
    pub(crate) conversions: i64,
    pub(crate) revenue_minor: i64,
    pub(crate) spend_minor: i64,
}

impl MetricTotals {
    pub(crate) fn add(&mut self, value: &MetricSnapshot) {
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
