use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hmac::{Hmac, KeyInit, Mac};
use reqwest::blocking::{Client, Response};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::secret;

pub fn weles_enqueue(
    base_url: &str,
    token_source: &str,
    account_id: &str,
    platform: &str,
    action: &str,
    params: Value,
) -> Result<Value> {
    required("Weles base URL", base_url)?;
    required("Weles account ID", account_id)?;
    required("Weles platform", platform)?;
    required("Weles action", action)?;
    let token = secret::read(token_source)?;
    let endpoint = format!(
        "{}/rest/v1/account_action_logs?select=id,status,platform,action,scheduled_at",
        base_url.trim_end_matches('/')
    );
    let payload = json!({
        "account_id": account_id,
        "platform": platform,
        "action": action,
        "status": "queued",
        "params": params,
    });
    let response = Client::new()
        .post(&endpoint)
        .header("apikey", &token)
        .bearer_auth(&token)
        .header("Prefer", "return=representation")
        .json(&payload)
        .send()
        .context("cannot enqueue Weles action")?;
    json_response(response, "Weles queue")
}

pub fn weles_status(base_url: &str, token_source: &str, job_id: &str) -> Result<Value> {
    required("Weles base URL", base_url)?;
    required("Weles job ID", job_id)?;
    let token = secret::read(token_source)?;
    let endpoint = format!(
        "{}/rest/v1/account_action_logs",
        base_url.trim_end_matches('/')
    );
    let response = Client::new().get(&endpoint)
        .header("apikey", &token)
        .bearer_auth(&token)
        .query(&[
            ("id", format!("eq.{job_id}")),
            ("select", "id,status,platform,action,params,result,error,scheduled_at,started_at,completed_at".into()),
        ])
        .send()
        .context("cannot read Weles action status")?;
    json_response(response, "Weles queue")
}

pub fn brama_health(base_url: &str) -> Result<Value> {
    let endpoint = format!("{}/health", base_url.trim_end_matches('/'));
    let response = Client::new()
        .get(&endpoint)
        .send()
        .context("cannot reach Brama")?;
    json_response(response, "Brama")
}

pub fn brama_analyze(
    base_url: &str,
    agent_id: &str,
    signing_secret_source: &str,
    model: &str,
    subject_kind: &str,
    subject: &Value,
    instruction: Option<&str>,
) -> Result<Value> {
    required("Brama base URL", base_url)?;
    required("Brama agent ID", agent_id)?;
    required("Brama model", model)?;
    let secret = secret::read(signing_secret_source)?;
    let system = system_prompt(subject_kind);
    let user = match instruction {
        Some(instruction) => format!(
            "Operator instruction:\n{instruction}\n\nCanonical {subject_kind} JSON:\n{}",
            serde_json::to_string_pretty(subject)?
        ),
        None => format!(
            "Review this canonical {subject_kind} JSON:\n{}",
            serde_json::to_string_pretty(subject)?
        ),
    };
    let payload = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    brama_post(
        base_url,
        agent_id,
        &secret,
        "/v1/chat/completions",
        &payload,
        "Brama analysis request failed",
    )
}

/// The alias Brama answers typed questions through; which model answers is
/// Brama's route.
const DECISION_ALIAS: &str = "decision-model";

/// Brama's typed answers to `questions` about `state`, keyed as asked, from
/// `POST /v1/decisions` signed as `agent_id`. An answer document without
/// `answers`, or a question left unanswered, is refused with what Brama sent.
pub fn brama_decide(
    base_url: &str,
    agent_id: &str,
    signing_secret_source: &str,
    state: &Value,
    questions: &Value,
) -> Result<serde_json::Map<String, Value>> {
    required("Brama base URL", base_url)?;
    required("Brama agent ID", agent_id)?;
    let secret = secret::read(signing_secret_source)?;
    let payload = json!({"model": DECISION_ALIAS, "state": state, "questions": questions});
    let answer = brama_post(
        base_url,
        agent_id,
        &secret,
        "/v1/decisions",
        &payload,
        "Brama decision request failed",
    )?;
    let answers = match answer.get("answers") {
        Some(Value::Object(answers)) => answers.clone(),
        _ => bail!("Brama's decision carries no answers object: {answer}"),
    };
    if let Some(missing) = questions
        .as_object()
        .into_iter()
        .flat_map(|asked| asked.keys())
        .find(|key| !answers.contains_key(*key))
    {
        bail!("Brama's decision answered no `{missing}`: {answer}");
    }
    Ok(answers)
}

/// POST `payload` to Brama's `path`, signed as `agent_id` with `secret` over
/// the body (`x-agent-signature` = HMAC-SHA256 of `agent:timestamp:sha256(body)`).
fn brama_post(
    base_url: &str,
    agent_id: &str,
    secret: &str,
    path: &str,
    payload: &Value,
    failed: &str,
) -> Result<Value> {
    let body = serde_json::to_vec(payload)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .to_string();
    let body_hash = hex::encode(Sha256::digest(&body));
    let signed = format!("{agent_id}:{timestamp}:{body_hash}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
    mac.update(signed.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    let endpoint = format!("{}{path}", base_url.trim_end_matches('/'));
    let response = Client::new()
        .post(&endpoint)
        .header("content-type", "application/json")
        .header("x-agent-id", agent_id)
        .header("x-agent-timestamp", &timestamp)
        .header("x-agent-signature", signature)
        .body(body)
        .send()
        .context(failed.to_string())?;
    json_response(response, "Brama")
}

/// The reviewer's instructions for a record of `kind`: the model reads the
/// kind and judges what a review of such a record weighs.
fn system_prompt(kind: &str) -> String {
    format!(
        "Review the supplied canonical UGC {kind} record as the operations reviewer a record of that kind needs, using only available evidence: do not invent product facts, never claim to have watched media when no frames or transcript are supplied, and name questions that need human or legal review rather than answering them as counsel. Return concise JSON with findings, risks, missing information and next actions."
    )
}

fn json_response(response: Response, service: &str) -> Result<Value> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        bail!("{service} returned {status}: {text}");
    }
    if text.trim().is_empty() {
        return Ok(json!({"status": "accepted"}));
    }
    serde_json::from_str(&text).with_context(|| format!("{service} returned invalid JSON"))
}

fn required(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} is required");
    }
    Ok(())
}
