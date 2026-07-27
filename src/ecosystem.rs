use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
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
    let body = serde_json::to_vec(&payload)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .to_string();
    let body_hash = hex::encode(Sha256::digest(&body));
    let signed = format!("{agent_id}:{timestamp}:{body_hash}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
    mac.update(signed.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    let endpoint = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let response = Client::new()
        .post(&endpoint)
        .header("content-type", "application/json")
        .header("x-agent-id", agent_id)
        .header("x-agent-timestamp", &timestamp)
        .header("x-agent-signature", signature)
        .body(body)
        .send()
        .context("Brama analysis request failed")?;
    json_response(response, "Brama")
}

fn system_prompt(kind: &str) -> &'static str {
    match kind {
        "brief" => {
            "You are a senior UGC creative strategist. Review the brief for ambiguity, missing shots, unsafe claims, weak hooks, platform fit, and testability. Return concise JSON with strengths, risks, and concrete changes. Do not invent product facts."
        }
        "submission" | "asset" => {
            "You are a UGC content reviewer. Evaluate only the supplied canonical metadata. Identify review risks, required human checks, and likely brief mismatches. Return concise JSON. Never claim to have watched media when no frames or transcript are supplied."
        }
        "rights" | "usage_rights" => {
            "You are a usage-rights operations reviewer, not legal counsel. Identify missing fields, expiry and channel conflicts, paid-media restrictions, and questions requiring human or legal review. Return concise JSON."
        }
        "creator" => {
            "You are a UGC creator-operations analyst. Evaluate fit using only supplied languages, markets, niches, identities, and metadata. Return concise JSON with fit signals, missing evidence, and risks."
        }
        "campaign" | "assignment" => {
            "You are a UGC campaign operations analyst. Review execution readiness, dependencies, timing, ownership, and inconsistencies using only supplied data. Return concise JSON with blockers and next actions."
        }
        _ => {
            "Review the supplied canonical UGC record using only available evidence. Return concise JSON with findings, risks, missing information, and next actions."
        }
    }
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
