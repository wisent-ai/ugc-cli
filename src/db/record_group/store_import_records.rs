use super::*;

impl Store {
    pub fn import_records(&self, records: &[Record], actor: &str) -> Result<Value> {
        if records.is_empty() {
            return Ok(serde_json::json!({
                "applied": false,
                "imported": [],
                "unchanged": [],
                "conflicting": [],
                "rejected": [{"kind": "$", "id": "", "reason": "record export is empty"}],
            }));
        }
        let existing = self.all_records()?;
        let mut by_id = std::collections::BTreeMap::new();
        let mut by_external = std::collections::BTreeMap::new();
        for record in existing {
            if let Some(external_id) = &record.external_id {
                by_external.insert(
                    (record.kind.clone(), external_id.clone()),
                    record.id.clone(),
                );
            }
            by_id.insert(record.id.clone(), record);
        }

        let mut incoming_ids = std::collections::BTreeSet::new();
        let mut incoming_external = std::collections::BTreeMap::new();
        let mut unchanged = Vec::new();
        let mut conflicting = Vec::new();
        let mut rejected = Vec::new();
        let mut pending = Vec::new();

        for record in records {
            if !incoming_ids.insert(record.id.clone()) {
                rejected.push(import_issue(record, "duplicate record id in import"));
                continue;
            }
            if let Some(external_id) = &record.external_id {
                let key = (record.kind.clone(), external_id.clone());
                if let Some(other_id) = incoming_external.insert(key, record.id.clone()) {
                    rejected.push(import_issue(
                        record,
                        &format!("external identity is also used by record {other_id}"),
                    ));
                    continue;
                }
            }
            if let Err(error) = validate_import_record(record) {
                rejected.push(import_issue(record, &error.to_string()));
                continue;
            }
            if let Some(current) = by_id.get(&record.id) {
                if current == record {
                    unchanged.push(record.id.clone());
                } else {
                    conflicting.push(import_issue(
                        record,
                        "destination already has a different record with this id",
                    ));
                }
                continue;
            }
            if let Some(external_id) = &record.external_id {
                if let Some(current_id) =
                    by_external.get(&(record.kind.clone(), external_id.clone()))
                {
                    conflicting.push(import_issue(
                        record,
                        &format!(
                            "destination record {current_id} already owns this external identity"
                        ),
                    ));
                    continue;
                }
            }
            pending.push(record);
        }
        let available_records: std::collections::BTreeMap<&str, &str> = by_id
            .iter()
            .map(|(id, record)| (id.as_str(), record.kind.as_str()))
            .chain(
                pending
                    .iter()
                    .map(|record| (record.id.as_str(), record.kind.as_str())),
            )
            .collect();
        for record in &pending {
            if record.kind == "provider_event" && record.parent_id.is_none() {
                rejected.push(import_issue(record, "provider event connection parent is missing"));
            }
            if let Some(parent_id) = record.parent_id.as_deref() {
                match available_records.get(parent_id) {
                    None => rejected.push(import_issue(
                        record,
                        &format!("parent record is missing: {parent_id}"),
                    )),
                    Some(actual_kind)
                        if expected_parent_kind(&record.kind)
                            .is_some_and(|expected| *actual_kind != expected) =>
                    {
                        rejected.push(import_issue(
                            record,
                            &format!(
                                "parent record {parent_id} is {actual_kind}, expected {}",
                                expected_parent_kind(&record.kind).unwrap_or_default()
                            ),
                        ));
                    }
                    Some(_) => {}
                }
            }
            if let Some(secondary_id) = record.secondary_id.as_deref() {
                match available_records.get(secondary_id) {
                    None if secondary_reference_is_record(&record.kind) => {
                        rejected.push(import_issue(
                            record,
                            &format!("secondary record is missing: {secondary_id}"),
                        ));
                    }
                    Some(actual_kind)
                        if expected_secondary_kind(&record.kind)
                            .is_some_and(|expected| *actual_kind != expected) =>
                    {
                        rejected.push(import_issue(
                            record,
                            &format!(
                                "secondary record {secondary_id} is {actual_kind}, expected {}",
                                expected_secondary_kind(&record.kind).unwrap_or_default()
                            ),
                        ));
                    }
                    _ => {}
                }
            }
        }

        if !conflicting.is_empty() || !rejected.is_empty() {
            return Ok(serde_json::json!({
                "applied": false,
                "imported": [],
                "unchanged": unchanged,
                "conflicting": conflicting,
                "rejected": rejected,
            }));
        }
        let first_success_missing = self
            .get_setting(crate::onboarding::FIRST_SUCCESS_KEY)?
            .is_none();

        let transaction = self.db.transaction()?;
        let imported_at = Self::now();
        for record in &pending {
            transaction.execute(
                r#"
                INSERT INTO records(kind,id,parent_id,secondary_id,status,external_id,data,created_at,updated_at)
                VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)
                "#,
                params![
                    record.kind,
                    record.id,
                    record.parent_id,
                    record.secondary_id,
                    record.status,
                    record.external_id,
                    record.data.to_string(),
                    record.created_at,
                    record.updated_at,
                ],
            )?;
            transaction.execute(
                "INSERT INTO audit_events(id,aggregate_type,aggregate_id,action,actor,details,created_at) VALUES($1,$2,$3,'imported',$4,$5,$6)",
                params![
                    Self::id(),
                    record.kind,
                    record.id,
                    actor,
                    serde_json::json!({"external_id": record.external_id}).to_string(),
                    imported_at,
                ],
            )?;
        }
        if first_success_missing {
            if let Some(campaign) = records.iter().find(|record| record.kind == "campaign") {
                transaction.execute(
                    "INSERT INTO settings(key,value,updated_at) VALUES($1,$2,$3)",
                    params![
                        crate::onboarding::FIRST_SUCCESS_KEY,
                        serde_json::json!({
                            "product_id": "ugc-cli",
                            "journey_id": "first-use",
                            "fact": "campaign_record_created",
                            "campaign_id": campaign.id,
                            "observed_at": campaign.created_at,
                        })
                        .to_string(),
                        imported_at,
                    ],
                )?;
            }
        }
        transaction.commit()?;

        Ok(serde_json::json!({
            "applied": true,
            "imported": pending.iter().map(|record| &record.id).collect::<Vec<_>>(),
            "unchanged": unchanged,
            "conflicting": [],
            "rejected": [],
        }))
    }
    pub fn counts(&self) -> Result<Value> {
        let mut stmt = self
            .db
            .prepare("SELECT kind,COUNT(*) AS count FROM records GROUP BY kind ORDER BY kind")?;
        let rows = stmt.query_map(params![], |row| {
            Ok((row.get::<_, String>("kind")?, row.get::<_, i64>("count")?))
        })?;
        let mut map = serde_json::Map::new();
        for row in rows {
            let (kind, count) = row?;
            map.insert(kind, Value::from(count));
        }
        Ok(Value::Object(map))
    }
    pub(crate) fn map_record(row: &Row<'_>) -> stado_database::sync::Result<Record> {
        let data: String = row.get("data")?;
        Ok(Record {
            kind: row.get("kind")?,
            id: row.get("id")?,
            parent_id: row.get("parent_id")?,
            secondary_id: row.get("secondary_id")?,
            status: row.get("status")?,
            external_id: row.get("external_id")?,
            data: serde_json::from_str(&data).unwrap_or(Value::String(data)),
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
    pub(crate) fn map_outbox(row: &Row<'_>) -> stado_database::sync::Result<OutboxItem> {
        let payload: String = row.get("payload")?;
        Ok(OutboxItem {
            id: row.get("id")?,
            kind: row.get("kind")?,
            aggregate_type: row.get("aggregate_type")?,
            aggregate_id: row.get("aggregate_id")?,
            connection_id: row.get("connection_id")?,
            payload: serde_json::from_str(&payload).unwrap_or(Value::String(payload)),
            status: row.get("status")?,
            attempts: row.get("attempts")?,
            available_at: row.get("available_at")?,
            last_error: row.get("last_error")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

pub(crate) fn import_issue(record: &Record, reason: &str) -> Value {
    serde_json::json!({
        "kind": record.kind,
        "id": record.id,
        "reason": reason,
    })
}
