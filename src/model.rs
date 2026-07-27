use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub campaign_create: bool,
    pub campaign_update: bool,
    pub creator_discovery: bool,
    pub applications: bool,
    pub messaging: bool,
    pub submissions: bool,
    pub revisions: bool,
    pub payments: bool,
    pub rights_metadata: bool,
    pub webhooks: bool,
    pub polling: bool,
}

impl ProviderCapabilities {
    pub fn manual() -> Self {
        Self {
            campaign_create: true,
            campaign_update: true,
            creator_discovery: false,
            applications: false,
            messaging: true,
            submissions: true,
            revisions: true,
            payments: true,
            rights_metadata: true,
            webhooks: false,
            polling: false,
        }
    }

    pub fn http() -> Self {
        Self {
            campaign_create: true,
            campaign_update: true,
            creator_discovery: true,
            applications: true,
            messaging: true,
            submissions: true,
            revisions: true,
            payments: true,
            rights_metadata: true,
            webhooks: true,
            polling: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub status: String,
    pub base_url: Option<String>,
    pub token_env: Option<String>,
    pub webhook_secret_env: Option<String>,
    pub external_account_id: Option<String>,
    pub capabilities: ProviderCapabilities,
    pub sync_cursor: Option<String>,
    pub last_sync_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub id: String,
    pub name: String,
    pub brand: String,
    pub product: String,
    pub objective: String,
    pub markets: Vec<String>,
    pub languages: Vec<String>,
    pub channels: Vec<String>,
    pub budget_minor: Option<i64>,
    pub currency: String,
    pub deadline: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brief {
    pub id: String,
    pub campaign_id: String,
    pub version: i64,
    pub service_type: String,
    pub creative_angle: String,
    pub requirements: Vec<String>,
    pub forbidden_claims: Vec<String>,
    pub required_shots: Vec<String>,
    pub talking_points: Vec<String>,
    pub cta: Option<String>,
    pub duration_min_ms: Option<i64>,
    pub duration_max_ms: Option<i64>,
    pub aspect_ratios: Vec<String>,
    pub raw_footage_required: bool,
    pub revision_limit: Option<i64>,
    pub rights_requirements: Value,
    pub status: String,
    pub approved_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Creator {
    pub id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub languages: Vec<String>,
    pub markets: Vec<String>,
    pub niches: Vec<String>,
    pub status: String,
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorIdentity {
    pub id: String,
    pub creator_id: String,
    pub connection_id: Option<String>,
    pub platform: String,
    pub external_creator_id: String,
    pub profile_url: Option<String>,
    pub metadata: Value,
    pub last_synced_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub id: String,
    pub campaign_id: String,
    pub brief_id: String,
    pub creator_id: String,
    pub connection_id: Option<String>,
    pub external_assignment_id: Option<String>,
    pub status: String,
    pub compensation_minor: Option<i64>,
    pub currency: String,
    pub payment_owner: String,
    pub deadline: Option<String>,
    pub shipping_required: bool,
    pub revision_limit: Option<i64>,
    pub accepted_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShippingAddress {
    pub recipient_name: String,
    pub line1: String,
    #[serde(default)]
    pub line2: Option<String>,
    pub city: String,
    #[serde(default)]
    pub region: Option<String>,
    pub postal_code: String,
    pub country: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shipment {
    pub id: String,
    pub assignment_id: String,
    pub status: String,
    pub carrier: Option<String>,
    pub tracking_number: Option<String>,
    pub product_variant: Option<String>,
    #[serde(default)]
    pub shipping_address: Option<ShippingAddress>,
    pub shipped_at: Option<String>,
    pub delivered_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Submission {
    pub id: String,
    pub assignment_id: String,
    pub external_submission_id: Option<String>,
    pub revision: i64,
    pub status: String,
    pub feedback: Option<String>,
    pub qc_status: Option<String>,
    pub qc_report: Option<Value>,
    pub submitted_at: String,
    pub reviewed_at: Option<String>,
    pub approved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub submission_id: Option<String>,
    pub role: String,
    pub source_url: Option<String>,
    pub local_path: String,
    pub sha256: String,
    pub mime_type: String,
    pub bytes: i64,
    pub duration_ms: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub metadata: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRights {
    pub id: String,
    pub assignment_id: String,
    pub asset_id: Option<String>,
    pub owner: String,
    pub license_type: String,
    pub organic_allowed: bool,
    pub paid_ads_allowed: bool,
    pub whitelisting_allowed: bool,
    pub editing_allowed: bool,
    pub ai_transform_allowed: bool,
    pub raw_footage_allowed: bool,
    pub territories: Vec<String>,
    pub channels: Vec<String>,
    pub starts_at: String,
    pub expires_at: Option<String>,
    pub model_release: bool,
    pub music_cleared: bool,
    pub contract_url: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
    pub id: String,
    pub assignment_id: String,
    pub submission_id: Option<String>,
    pub owner: String,
    pub amount_minor: i64,
    pub currency: String,
    pub status: String,
    pub external_payment_id: Option<String>,
    pub idempotency_key: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub assignment_id: String,
    pub direction: String,
    pub channel: String,
    pub body: String,
    pub external_message_id: Option<String>,
    pub created_at: String,
}

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
