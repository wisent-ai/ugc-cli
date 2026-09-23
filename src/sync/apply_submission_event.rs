use super::*;

pub(crate) fn apply_submission_event(store: &Store, event: &ProviderEvent) -> Result<()> {
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

pub(crate) fn apply_payment_event(store: &Store, event: &ProviderEvent) -> Result<()> {
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
