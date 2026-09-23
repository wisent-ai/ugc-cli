use super::*;

pub(crate) fn classify_intent(body: &str) -> String {
    let text = body.to_lowercase();
    let contains = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if contains(&["unsubscribe", "stop", "remove me", "nie pisz", "wypisz"]) {
        "opt_out"
    } else if contains(&[
        "not interested",
        "no thanks",
        "decline",
        "odmawiam",
        "nie jestem zainteres",
    ]) {
        "declined"
    } else if contains(&[
        "accept",
        "i agree",
        "confirmed",
        "akceptuję",
        "akceptuje",
        "zgadzam się",
    ]) {
        "accepted"
    } else if contains(&[
        "interested",
        "sounds good",
        "zainteresowan",
        "chętnie",
        "chetnie",
    ]) {
        "interested"
    } else if contains(&[
        "price",
        "rate",
        "budget",
        "fee",
        "stawka",
        "cena",
        "wynagrodzenie",
    ]) {
        "pricing"
    } else if contains(&[
        "submitted",
        "uploaded",
        "delivered",
        "wysł",
        "gotowe",
        "przesł",
    ]) {
        "submitted"
    } else if text.contains('?') {
        "question"
    } else {
        "other"
    }
    .into()
}

pub(crate) fn score_filter(
    expected: &[String],
    actual: &[String],
    label: &str,
    weight: i64,
    score: &mut i64,
    matched: &mut Vec<String>,
    missing: &mut Vec<String>,
) {
    if expected.is_empty() {
        return;
    }
    if overlap(expected, actual) {
        *score += weight;
        matched.push(label.into());
    } else {
        missing.push(label.into());
    }
}

pub(crate) fn overlap(left: &[String], right: &[String]) -> bool {
    left.iter()
        .any(|left| right.iter().any(|right| left.eq_ignore_ascii_case(right)))
}

pub(crate) fn metadata_overlap(metadata: &Value, key: &str, expected: &[String]) -> bool {
    metadata
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values.iter().filter_map(Value::as_str).any(|value| {
                expected
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(value))
            })
        })
}

pub(crate) fn metadata_i64(metadata: &Value, key: &str) -> Option<i64> {
    metadata
        .get(key)
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
}

pub(crate) fn metadata_f64(metadata: &Value, key: &str) -> Option<f64> {
    metadata
        .get(key)
        .and_then(|value| value.as_f64().or_else(|| value.as_str()?.parse().ok()))
}

pub(crate) fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub(crate) fn ratio(numerator: i64, denominator: i64) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

pub(crate) fn is_conversion_event(event_type: &str) -> bool {
    matches!(
        event_type.to_ascii_lowercase().as_str(),
        "conversion" | "order" | "purchase" | "sale"
    )
}

pub(crate) fn unique(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
