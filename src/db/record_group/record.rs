use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub kind: String,
    pub id: String,
    pub parent_id: Option<String>,
    pub secondary_id: Option<String>,
    pub status: String,
    pub external_id: Option<String>,
    pub data: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    pub id: String,
    pub kind: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub connection_id: Option<String>,
    pub payload: Value,
    pub status: String,
    pub attempts: i64,
    pub available_at: String,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// The UGC ledger, held in the fleet database `ugc-cli`.
pub struct Store {
    pub(crate) db: Client,
}

/// Bind a list of text parameters of a length only known at run time.
pub(crate) fn texts(values: &[String]) -> Values {
    Values(values.iter().map(Bind::bind).collect())
}
