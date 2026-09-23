use super::*;

impl<'a> StandaloneService<'a> {
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
        if offered_compensation_minor.is_some_and(|amount| amount <= 0) {
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
        if compensation <= 0 {
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
        if days.is_some_and(|days| days <= 0) {
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
}
