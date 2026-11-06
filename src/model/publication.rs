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

/// The verdict of technical QC, of one check and of the whole report: the
/// report carries the worst verdict any of its checks reached, so the order
/// of the cases (pass, then warn, then fail) is the order of severity. A
/// stored report naming another verdict is refused when it is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum QcStatus {
    Pass,
    Warn,
    Fail,
}

impl QcStatus {
    /// Whether a submission carrying this verdict may be approved: a warning
    /// is reviewed by a person, a failure is not.
    pub fn approvable(self) -> bool {
        self != Self::Fail
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcReport {
    pub status: QcStatus,
    pub checks: Vec<QcCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcCheck {
    pub name: String,
    pub status: QcStatus,
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
    /// Engagement rate from which a creator's audience counts as evidence; unset, the
    /// rate is reported and not ranked.
    #[serde(default)]
    pub min_engagement_rate: Option<f64>,
    /// Response rate from which a creator counts as one who answers; unset, the rate
    /// is reported and not ranked.
    #[serde(default)]
    pub min_response_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorMatch {
    pub creator: Creator,
    pub score: i64,
    pub matched: Vec<String>,
    pub missing: Vec<String>,
    pub signals: Value,
}

/// Where a creator conversation stands: open from outreach, then interested,
/// accepted (an assignment exists), declined or opted out, as the creator's
/// replies say. `closed` is read from records stored before the declined and
/// opted-out states existed. A stored conversation naming any other status is
/// refused when it is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
    Open,
    Interested,
    Accepted,
    Declined,
    OptedOut,
    Closed,
}

impl ConversationStatus {
    /// The status as the store's status column holds it.
    pub fn word(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Interested => "interested",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::OptedOut => "opted_out",
            Self::Closed => "closed",
        }
    }

    /// Whether the creator ended the conversation: nothing more is sent or
    /// received, its offer reserves no budget, and it is never accepted.
    pub fn ended(self) -> bool {
        matches!(self, Self::Declined | Self::OptedOut | Self::Closed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub creator_id: String,
    pub campaign_id: Option<String>,
    pub brief_id: Option<String>,
    pub assignment_id: Option<String>,
    pub status: ConversationStatus,
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

/// Whether a creator's portal link still opens the portal: active from its
/// creation until the operator revokes it. A stored access naming another
/// status is refused when it is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortalAccessStatus {
    Active,
    Revoked,
}

impl PortalAccessStatus {
    /// The status as the store's status column holds it.
    pub fn word(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalAccess {
    pub id: String,
    pub creator_id: String,
    pub token_hash: String,
    pub status: PortalAccessStatus,
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
