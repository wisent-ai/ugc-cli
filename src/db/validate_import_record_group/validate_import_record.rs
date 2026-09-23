use super::*;

pub(crate) fn validate_import_record(record: &Record) -> Result<()> {
    if record.kind.trim().is_empty()
        || record.id.trim().is_empty()
        || record.status.trim().is_empty()
    {
        bail!("import record kind, id, and status are required");
    }
    chrono::DateTime::parse_from_rfc3339(&record.created_at)
        .context("record created_at must be RFC 3339")?;
    chrono::DateTime::parse_from_rfc3339(&record.updated_at)
        .context("record updated_at must be RFC 3339")?;

    let canonical = match record.kind.as_str() {
        "connection" => canonical_import_data::<Connection>(record)?,
        "campaign" => canonical_import_data::<Campaign>(record)?,
        "brief" => canonical_import_data::<Brief>(record)?,
        "creator" => canonical_import_data::<Creator>(record)?,
        "creator_identity" => canonical_import_data::<CreatorIdentity>(record)?,
        "assignment" => canonical_import_data::<Assignment>(record)?,
        "shipment" => canonical_import_data::<Shipment>(record)?,
        "submission" => canonical_import_data::<Submission>(record)?,
        "asset" => canonical_import_data::<Asset>(record)?,
        "usage_rights" => canonical_import_data::<UsageRights>(record)?,
        "payment" => canonical_import_data::<Payment>(record)?,
        "message" => canonical_import_data::<Message>(record)?,
        "publication" => canonical_import_data::<Publication>(record)?,
        "provider_event" => canonical_import_data::<ProviderEvent>(record)?,
        "portal_access" => canonical_import_data::<PortalAccess>(record)?,
        "standalone_publication" => {
            canonical_import_data::<StandalonePublication>(record)?
        }
        "metric_snapshot" => canonical_import_data::<MetricSnapshot>(record)?,
        "attribution_event" => canonical_import_data::<AttributionEvent>(record)?,
        "conversation" => canonical_import_data::<Conversation>(record)?,
        "conversation_message" => canonical_import_data::<ConversationMessage>(record)?,
        "ledger_transfer" => canonical_import_data::<LedgerTransfer>(record)?,
        other => bail!("unsupported import record kind: {other}"),
    };
    if canonical != record.data {
        bail!("record data has unsupported, missing, or non-canonical fields");
    }
    if record.data.get("id").and_then(Value::as_str) != Some(record.id.as_str()) {
        bail!("record id does not match data.id");
    }
    if let Some(status) = record.data.get("status").and_then(Value::as_str) {
        if status != record.status {
            bail!("record status does not match data.status");
        }
    }
    if let Some(created_at) = record.data.get("created_at").and_then(Value::as_str) {
        if created_at != record.created_at {
            bail!("record created_at does not match data.created_at");
        }
    }
    if let Some(updated_at) = record.data.get("updated_at").and_then(Value::as_str) {
        if updated_at != record.updated_at {
            bail!("record updated_at does not match data.updated_at");
        }
    }
    if let Some(parent_field) = parent_field(&record.kind) {
        let data_parent = record.data.get(parent_field).and_then(Value::as_str);
        if data_parent != record.parent_id.as_deref() {
            bail!("record parent_id does not match data.{parent_field}");
        }
    }
    if let Some(secondary_field) = secondary_field(&record.kind) {
        let data_secondary = record.data.get(secondary_field).and_then(Value::as_str);
        if data_secondary != record.secondary_id.as_deref() {
            bail!("record secondary_id does not match data.{secondary_field}");
        }
    }
    Ok(())
}

pub(crate) fn canonical_import_data<T>(record: &Record) -> Result<Value>
where
    T: DeserializeOwned + Serialize,
{
    let value: T = serde_json::from_value(record.data.clone())
        .with_context(|| format!("invalid {} record data", record.kind))?;
    serde_json::to_value(value).map_err(Into::into)
}

pub(crate) fn parent_field(kind: &str) -> Option<&'static str> {
    match kind {
        "asset" => Some("submission_id"),
        "brief" | "publication" | "standalone_publication" => Some("campaign_id"),
        "conversation" => Some("campaign_id"),
        "creator_identity" | "portal_access" => Some("creator_id"),
        "assignment" => Some("campaign_id"),
        "shipment" | "submission" | "usage_rights" | "payment" | "message"
        | "ledger_transfer" => Some("assignment_id"),
        "metric_snapshot" | "attribution_event" => Some("publication_id"),
        "conversation_message" => Some("conversation_id"),
        _ => None,
    }
}

pub(crate) fn secondary_field(kind: &str) -> Option<&'static str> {
    match kind {
        "assignment" | "conversation" | "conversation_message" => Some("creator_id"),
        "usage_rights" => Some("asset_id"),
        "publication" => Some("connection_id"),
        "standalone_publication" => Some("assignment_id"),
        _ => None,
    }
}

pub(crate) fn secondary_reference_is_record(kind: &str) -> bool {
    secondary_field(kind).is_some()
}

pub(crate) fn expected_parent_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "brief" | "assignment" | "publication" | "standalone_publication"
        | "conversation" => Some("campaign"),
        "creator_identity" | "portal_access" => Some("creator"),
        "shipment" | "submission" | "usage_rights" | "payment" | "message"
        | "ledger_transfer" => Some("assignment"),
        "asset" => Some("submission"),
        "metric_snapshot" | "attribution_event" => Some("standalone_publication"),
        "conversation_message" => Some("conversation"),
        "provider_event" => Some("connection"),
        _ => None,
    }
}

pub(crate) fn expected_secondary_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "assignment" | "conversation" | "conversation_message" => Some("creator"),
        "usage_rights" => Some("asset"),
        "publication" => Some("connection"),
        "standalone_publication" => Some("assignment"),
        _ => None,
    }
}

#[cfg(unix)]
pub(crate) fn protect_database_files(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    const OWNER_ONLY_FILE_MODE: u32 = 0o600;
    for target in [
        path.to_path_buf(),
        database_sidecar(path, "-wal"),
        database_sidecar(path, "-shm"),
    ] {
        if target.exists() {
            fs::set_permissions(&target, fs::Permissions::from_mode(OWNER_ONLY_FILE_MODE))
                .with_context(|| format!("cannot protect {}", target.display()))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn database_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(not(unix))]
pub(crate) fn protect_database_files(_path: &Path) -> Result<()> {
    Ok(())
}
