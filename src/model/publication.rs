use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Publication {
    pub id: String,
    pub campaign_id: String,
    pub brief_id: String,
    pub connection_id: String,
    pub external_campaign_id: Option<String>,
    pub external_url: Option<String>,
    pub status: String,
    pub provider_status: Option<String>,
    pub last_synced_at: Option<String>,
    pub sync_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcReport {
    pub status: String,
    pub checks: Vec<QcCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcCheck {
    pub name: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEvent {
    pub external_id: String,
    pub kind: String,
    pub aggregate_type: String,
    pub aggregate_external_id: String,
    pub payload: Value,
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryQuery {
    pub campaign_id: Option<String>,
    pub markets: Vec<String>,
    pub languages: Vec<String>,
    pub niches: Vec<String>,
    pub channels: Vec<String>,
    pub min_followers: Option<i64>,
    pub max_rate_minor: Option<i64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorMatch {
    pub creator: Creator,
    pub score: i64,
    pub matched: Vec<String>,
    pub missing: Vec<String>,
    pub signals: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub creator_id: String,
    pub campaign_id: Option<String>,
    pub brief_id: Option<String>,
    pub assignment_id: Option<String>,
    pub status: String,
    pub stage: String,
    pub offered_compensation_minor: Option<i64>,
    pub currency: String,
    #[serde(default)]
    pub shipping_required: bool,
    pub next_action_at: Option<String>,
    pub last_inbound_at: Option<String>,
    pub last_outbound_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub id: String,
    pub conversation_id: String,
    pub creator_id: String,
    pub direction: String,
    pub channel: String,
    pub body: String,
    pub intent: Option<String>,
    pub automated: bool,
    pub external_message_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalAccess {
    pub id: String,
    pub creator_id: String,
    pub token_hash: String,
    pub status: String,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerTransfer {
    pub id: String,
    pub from_account: String,
    pub to_account: String,
    pub amount_minor: i64,
    pub currency: String,
    pub kind: String,
    pub status: String,
    pub assignment_id: Option<String>,
    pub payment_id: Option<String>,
    pub reference: Option<String>,
    pub idempotency_key: String,
    pub reversal_of: Option<String>,
    pub created_at: String,
    pub posted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerBalance {
    pub account: String,
    pub currency: String,
    pub balance_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandalonePublication {
    pub id: String,
    pub campaign_id: String,
    pub assignment_id: String,
    pub creator_id: String,
    pub submission_id: String,
    pub asset_id: String,
    pub platform: String,
    pub channel: String,
    #[serde(default)]
    pub territory: Option<String>,
    pub post_id: Option<String>,
    pub url: String,
    pub tracking_code: String,
    pub paid: bool,
    pub status: String,
    pub published_at: String,
    pub last_checked_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSnapshot {
    pub id: String,
    pub publication_id: String,
    pub captured_at: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionEvent {
    pub id: String,
    pub publication_id: String,
    pub event_type: String,
    pub external_event_id: Option<String>,
    pub value_minor: Option<i64>,
    pub currency: Option<String>,
    pub metadata: Value,
    pub occurred_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowReport {
    pub campaign_id: String,
    pub status: String,
    pub blockers: Vec<String>,
    pub actions: Vec<String>,
    pub counts: Value,
    pub updated_at: String,
}
