use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::{
    db::{OutboxItem, Store},
    model::{Assignment, Connection, Payment, ProviderEvent, Publication, Submission},
    provider,
};

pub fn process_outbox(store: &Store, limit: usize, actor: &str) -> Result<Value> {
    let items = store.due_outbox(limit)?;
    let mut completed = Vec::new();
    let mut failed = Vec::new();
    for item in items {
        match process_item(store, &item, actor) {
            Ok(result) => {
                store.complete_outbox(&item.id)?;
                completed.push(json!({"id": item.id, "result": result}));
            }
            Err(error) => {
                let attempts = item.attempts + "x".len() as i64;
                let terminal = attempts >= "retry".len() as i64;
                store.fail_outbox(&item.id, attempts, &error.to_string(), terminal)?;
                failed
                    .push(json!({"id": item.id, "error": error.to_string(), "terminal": terminal}));
            }
        }
    }
    Ok(json!({"completed": completed, "failed": failed}))
}

fn process_item(store: &Store, item: &OutboxItem, actor: &str) -> Result<Value> {
    let connection_id = item
        .connection_id
        .as_deref()
        .context("outbox item has no connection")?;
    let connection: Connection = store.get("connection", connection_id)?;
    if connection.status != "active" {
        bail!("connection {} is not active", connection.id);
    }
    let adapter = provider::adapter(&connection)?;
    let result = match item.kind.as_str() {
        "publish_campaign" => {
            let result = adapter.publish_campaign(&item.payload)?;
            let mut publication: Publication = store.get("publication", &item.aggregate_id)?;
            let external_id = result
                .get("external_campaign_id")
                .and_then(Value::as_str)
                .context("provider response has no external_campaign_id")?;
            publication.external_campaign_id = Some(external_id.into());
            publication.external_url = result
                .get("external_url")
                .and_then(Value::as_str)
                .map(str::to_owned);
            publication.provider_status = result
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_owned);
            publication.status = if connection.provider == "manual" {
                "manual_action_required".into()
            } else {
                "published".into()
            };
            publication.sync_error = None;
            publication.last_synced_at = Some(Store::now());
            publication.updated_at = Store::now();
            store.put(
                "publication",
                &publication.id,
                Some(&publication.campaign_id),
                Some(&publication.connection_id),
                &publication.status,
                publication.external_campaign_id.as_deref(),
                &publication,
                &publication.created_at,
            )?;
            store.audit("publication", &publication.id, "published", actor, &result)?;
            result
        }
        "send_message" => {
            let result = adapter.send_message(&item.payload)?;
            store.audit(
                "message",
                &item.aggregate_id,
                "provider_delivery",
                actor,
                &result,
            )?;
            result
        }
        "request_revision" => {
            let result = adapter.request_revision(&item.payload)?;
            store.audit(
                "submission",
                &item.aggregate_id,
                "provider_revision_request",
                actor,
                &result,
            )?;
            result
        }
        other => bail!("unsupported outbox kind: {other}"),
    };
    Ok(result)
}

pub fn sync_connection(store: &Store, connection_id: &str, actor: &str) -> Result<Value> {
    let mut connection: Connection = store.get("connection", connection_id)?;
    let adapter = provider::adapter(&connection)?;
    let (events, next_cursor) = adapter.poll(connection.sync_cursor.as_deref())?;
    let mut applied = Vec::new();
    let mut errors = Vec::new();
    for event in events {
        match apply_event(store, connection_id, &event, actor) {
            Ok(result) => applied.push(result),
            Err(error) => {
                errors.push(json!({"external_id": event.external_id, "error": error.to_string()}))
            }
        }
    }
    connection.sync_cursor = next_cursor;
    connection.last_sync_at = Some(Store::now());
    connection.updated_at = Store::now();
    store.put(
        "connection",
        &connection.id,
        None,
        None,
        &connection.status,
        None,
        &connection,
        &connection.created_at,
    )?;
    Ok(
        json!({"connection_id": connection_id, "applied": applied, "errors": errors, "cursor": connection.sync_cursor}),
    )
}

pub fn ingest_webhook(
    store: &Store,
    connection_id: &str,
    body: &[u8],
    signature: Option<&str>,
    actor: &str,
) -> Result<Value> {
    let connection: Connection = store.get("connection", connection_id)?;
    let adapter = provider::adapter(&connection)?;
    adapter.verify_webhook(body, signature)?;
    let event: ProviderEvent =
        serde_json::from_slice(body).context("webhook body does not match ProviderEvent")?;
    let inserted = store.store_webhook(
        connection_id,
        &event.external_id,
        &event.kind,
        &event.payload,
        true,
    )?;
    if !inserted {
        return Ok(json!({"duplicate": true, "provider_event_id": event.external_id}));
    }
    match apply_event(store, connection_id, &event, actor) {
        Ok(result) => {
            store.finish_webhook(connection_id, &event.external_id, None)?;
            Ok(json!({"duplicate": false, "result": result}))
        }
        Err(error) => {
            store.finish_webhook(connection_id, &event.external_id, Some(&error.to_string()))?;
            Err(error)
        }
    }
}

pub fn apply_event(
    store: &Store,
    connection_id: &str,
    event: &ProviderEvent,
    actor: &str,
) -> Result<Value> {
    if let Some(existing) = store.find_external::<Value>(
        "provider_event",
        &format!("{connection_id}:{}", event.external_id),
    )? {
        return Ok(json!({"duplicate": true, "event": existing}));
    }
    match event.aggregate_type.as_str() {
        "campaign" | "publication" => apply_publication_event(store, event)?,
        "assignment" => apply_assignment_event(store, event)?,
        "submission" => apply_submission_event(store, event)?,
        "payment" => apply_payment_event(store, event)?,
        other => {
            store.audit(
                "provider_event",
                &event.external_id,
                "unmapped",
                actor,
                &json!({"aggregate_type": other, "payload": event.payload}),
            )?;
        }
    }
    let id = Store::id();
    let external = format!("{connection_id}:{}", event.external_id);
    store.put(
        "provider_event",
        &id,
        Some(connection_id),
        None,
        "applied",
        Some(&external),
        event,
        &Store::now(),
    )?;
    store.audit(
        &event.aggregate_type,
        &event.aggregate_external_id,
        "provider_event_applied",
        actor,
        &json!({"event_id": event.external_id, "kind": event.kind}),
    )?;
    Ok(
        json!({"duplicate": false, "event_id": event.external_id, "aggregate_type": event.aggregate_type}),
    )
}

fn apply_publication_event(store: &Store, event: &ProviderEvent) -> Result<()> {
    let mut publication: Publication = store
        .find_external("publication", &event.aggregate_external_id)?
        .context("publication event references unknown external campaign")?;
    publication.provider_status = event
        .payload
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or(publication.provider_status);
    publication.last_synced_at = Some(Store::now());
    publication.updated_at = Store::now();
    store.put(
        "publication",
        &publication.id,
        Some(&publication.campaign_id),
        Some(&publication.connection_id),
        &publication.status,
        publication.external_campaign_id.as_deref(),
        &publication,
        &publication.created_at,
    )
}

fn apply_assignment_event(store: &Store, event: &ProviderEvent) -> Result<()> {
    let mut assignment: Assignment =
        match store.find_external("assignment", &event.aggregate_external_id)? {
            Some(item) => item,
            None => serde_json::from_value(
                event
                    .payload
                    .get("canonical")
                    .cloned()
                    .context("new assignment event needs payload.canonical")?,
            )?,
        };
    assignment.external_assignment_id = Some(event.aggregate_external_id.clone());
    if let Some(status) = event.payload.get("status").and_then(Value::as_str) {
        assignment.status = status.into();
    }
    assignment.updated_at = Store::now();
    store.put(
        "assignment",
        &assignment.id,
        Some(&assignment.campaign_id),
        Some(&assignment.creator_id),
        &assignment.status,
        assignment.external_assignment_id.as_deref(),
        &assignment,
        &assignment.created_at,
    )
}

fn apply_submission_event(store: &Store, event: &ProviderEvent) -> Result<()> {
    let mut submission: Submission =
        match store.find_external("submission", &event.aggregate_external_id)? {
            Some(item) => item,
            None => serde_json::from_value(
                event
                    .payload
                    .get("canonical")
                    .cloned()
                    .context("new submission event needs payload.canonical")?,
            )?,
        };
    submission.external_submission_id = Some(event.aggregate_external_id.clone());
    if let Some(status) = event.payload.get("status").and_then(Value::as_str) {
        submission.status = status.into();
    }
    store.put(
        "submission",
        &submission.id,
        Some(&submission.assignment_id),
        None,
        &submission.status,
        submission.external_submission_id.as_deref(),
        &submission,
        &submission.submitted_at,
    )
}

fn apply_payment_event(store: &Store, event: &ProviderEvent) -> Result<()> {
    let mut payment: Payment = store
        .find_external("payment", &event.aggregate_external_id)?
        .or_else(|| {
            event
                .payload
                .get("canonical")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
        })
        .context("payment event references unknown payment and has no canonical payload")?;
    payment.external_payment_id = Some(event.aggregate_external_id.clone());
    if let Some(status) = event.payload.get("status").and_then(Value::as_str) {
        payment.status = status.into();
    }
    payment.updated_at = Store::now();
    store.put(
        "payment",
        &payment.id,
        Some(&payment.assignment_id),
        None,
        &payment.status,
        Some(&payment.idempotency_key),
        &payment,
        &payment.created_at,
    )
}
