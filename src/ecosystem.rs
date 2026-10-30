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

/// The consumer ugc asks Stado's capability resolution as.
const CONSUMER: &str = "ugc";

/// Which model service answers ugc, and how it is called.
pub enum ModelService {
    /// Brama at `base_url`; every request it serves for a model is signed as
    /// the agent with its secret, a health read with nothing.
    Brama {
        base_url: String,
        signing: Option<(String, String)>,
    },
    /// The OpenAI-compatible endpoint Stado's capability resolution hands out
    /// in Brama's place: its own declared `model`, and the key of the role it
    /// names, when it names one. It is not Brama and takes no signature.
    Endpoint {
        provider: String,
        base_url: String,
        model: String,
        key: Option<String>,
    },
}

/// A setting read only when it is needed: Brama's agent id or signing secret
/// source, read only when Brama is the service that answers.
pub type Deferred<'a> = Box<dyn FnOnce() -> Result<String> + 'a>;

impl ModelService {
    /// The model service ugc calls. A base URL the operator names (`--base-url`
    /// or `BRAMA_URL`) is Brama, used as given. Otherwise ugc never dials Brama
    /// itself: it asks `stado capabilities resolve inference --consumer ugc`,
    /// which hands out Brama's route or the endpoint declared in its place.
    /// Brama's signing settings are read only when Brama is the one that
    /// answers and `signing` is given; a health read signs nothing. When no
    /// alternative answers, the refusal names each one with its route and cause.
    pub fn resolve(
        named: Option<String>,
        signing: Option<(Deferred<'_>, Deferred<'_>)>,
    ) -> Result<Self> {
        let named = named.filter(|url| !url.trim().is_empty());
        let base_url = match named {
            Some(base_url) => base_url,
            None => {
                let resolved = resolve_inference()?;
                if resolved.provider != "brama" {
                    let model = resolved.model.with_context(|| {
                        format!(
                            "{} at {} answers in Brama's place but declares no model: set capabilities.inference.endpoint.model",
                            resolved.provider, resolved.route
                        )
                    })?;
                    let key = match &resolved.key_role {
                        Some(role) => Some(role_key(role, &resolved.provider, &resolved.route)?),
                        None => None,
                    };
                    return Ok(ModelService::Endpoint {
                        provider: resolved.provider,
                        base_url: resolved.route,
                        model,
                        key,
                    });
                }
                resolved.route
            }
        };
        let signing = match signing {
            Some((agent_id, signing_secret_source)) => {
                let agent_id = agent_id()?;
                required("Brama agent ID", &agent_id)?;
                Some((agent_id, secret::read(&signing_secret_source()?)?))
            }
            None => None,
        };
        Ok(ModelService::Brama { base_url, signing })
    }

    /// The provider that answers, as reports name it.
    pub fn provider(&self) -> &str {
        match self {
            ModelService::Brama { .. } => "brama",
            ModelService::Endpoint { provider, .. } => provider,
        }
    }

    fn base_url(&self) -> &str {
        match self {
            ModelService::Brama { base_url, .. } | ModelService::Endpoint { base_url, .. } => {
                base_url.trim_end_matches('/')
            }
        }
    }

    /// POST `payload` to `path`: signed over the body for Brama
    /// (`x-agent-signature` = HMAC-SHA256 of `agent:timestamp:sha256(body)`),
    /// with the endpoint's key as a bearer otherwise.
    fn post(&self, path: &str, payload: &Value, failed: &str) -> Result<Value> {
        let body = serde_json::to_vec(payload)?;
        let endpoint = format!("{}{path}", self.base_url());
        let request = Client::new()
            .post(&endpoint)
            .header("content-type", "application/json");
        let request = match self {
            ModelService::Brama {
                signing: Some((agent_id, secret)),
                ..
            } => {
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .to_string();
                let body_hash = hex::encode(Sha256::digest(&body));
                let signed = format!("{agent_id}:{timestamp}:{body_hash}");
                let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
                mac.update(signed.as_bytes());
                let signature = hex::encode(mac.finalize().into_bytes());
                request
                    .header("x-agent-id", agent_id)
                    .header("x-agent-timestamp", &timestamp)
                    .header("x-agent-signature", signature)
            }
            ModelService::Brama { signing: None, .. } => {
                bail!(
                    "Brama answers only requests signed as an agent; set UGC_BRAMA_AGENT_ID and UGC_BRAMA_SIGNING_SECRET_SOURCE"
                )
            }
            ModelService::Endpoint { key: Some(key), .. } => request.bearer_auth(key),
            ModelService::Endpoint { key: None, .. } => request,
        };
        let response = request
            .body(body)
            .send()
            .with_context(|| format!("{failed} at {endpoint}"))?;
        json_response(response, self.provider())
    }
}

/// What `stado capabilities resolve inference --consumer ugc` handed out.
struct Resolved {
    provider: String,
    route: String,
    model: Option<String>,
    key_role: Option<String>,
}

fn resolve_inference() -> Result<Resolved> {
    let operation = format!("stado capabilities resolve inference --consumer {CONSUMER} --json");
    let output = std::process::Command::new("stado")
        .args(["capabilities", "resolve", "inference", "--consumer", CONSUMER, "--json"])
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| {
            format!("BRAMA_URL is not set and `{operation}` could not run; set BRAMA_URL or install Stado")
        })?;
    if !output.status.success() {
        let named = [&output.stdout, &output.stderr]
            .into_iter()
            .filter_map(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            .find_map(|envelope| envelope["details"]["unreachable"].as_array().cloned())
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        format!(
                            "{} at {}: {}",
                            item["product"].as_str().unwrap_or("an alternative"),
                            item["route"].as_str().unwrap_or("its route"),
                            item["cause"].as_str().unwrap_or("no cause given")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .filter(|named| !named.is_empty());
        match named {
            Some(named) => bail!(
                "BRAMA_URL is not set and no inference alternative answers {CONSUMER}: {named}; set BRAMA_URL, or declare an endpoint in Brama's place (stado config set capabilities.inference.endpoint.url <url> and capabilities.inference.endpoint.model <model>)"
            ),
            None => bail!(
                "BRAMA_URL is not set and `{operation}` exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }
    }
    let answer: Value = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("`{operation}` printed an answer that is not JSON"))?;
    let text = |field: &str| {
        answer[field]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let (Some(provider), Some(route)) = (text("provider"), text("route")) else {
        bail!("`{operation}` answered no provider and route");
    };
    Ok(Resolved {
        provider,
        route,
        model: text("model"),
        key_role: text("key_role"),
    })
}

/// The key of the endpoint Stado handed out, read as the role it names.
fn role_key(role: &str, provider: &str, base: &str) -> Result<String> {
    let output = std::process::Command::new("stado")
        .args(["credentials", "get", "--role", role, "--field", "token"])
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("stado credentials get --role {role} could not run"))?;
    if !output.status.success() {
        bail!(
            "{provider} at {base} answers in Brama's place, but the key of role {role} cannot be read: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let key = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if key.is_empty() {
        bail!("stado credentials get --role {role} answered an empty key");
    }
    Ok(key)
}

/// Whether the model service answers: Brama's `/health`, or an endpoint's
/// `/v1/models`, each named with the provider that answered.
pub fn model_health(service: &ModelService) -> Result<Value> {
    let (path, request) = match service {
        ModelService::Brama { .. } => (
            "/health",
            Client::new().get(format!("{}/health", service.base_url())),
        ),
        ModelService::Endpoint { key, .. } => {
            let request = Client::new().get(format!("{}/v1/models", service.base_url()));
            (
                "/v1/models",
                match key {
                    Some(key) => request.bearer_auth(key),
                    None => request,
                },
            )
        }
    };
    let response = request.send().with_context(|| {
        format!(
            "cannot reach {} at {}{path}",
            service.provider(),
            service.base_url()
        )
    })?;
    let answer = json_response(response, service.provider())?;
    Ok(json!({
        "provider": service.provider(),
        "base_url": service.base_url(),
        "answer": answer,
    }))
}

pub fn model_analyze(
    service: &ModelService,
    model: &str,
    subject_kind: &str,
    subject: &Value,
    instruction: Option<&str>,
) -> Result<Value> {
    let model = match service {
        ModelService::Brama { .. } => {
            required("Brama model", model)?;
            model
        }
        // The endpoint knows no Brama route: it is asked for its own model.
        ModelService::Endpoint { model, .. } => model.as_str(),
    };
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
    service.post(
        "/v1/chat/completions",
        &payload,
        "the analysis request failed",
    )
}

/// The alias Brama answers typed questions through; which model answers is
/// Brama's route.
const DECISION_ALIAS: &str = "decision-model";

/// One typed choice about `state`: `question` with `criteria` (option name to
/// meaning). Brama answers it through `POST /v1/decisions`; an endpoint in its
/// place, which has no decision route, is asked the same question as one chat
/// completion whose whole answer must be `{"choice": "<option>"}`. Either
/// answer that is not one of the options, or not that shape, is refused with
/// what the service sent.
pub fn model_choose(
    service: &ModelService,
    state: &Value,
    key: &str,
    instructions: &str,
    criteria: &serde_json::Map<String, Value>,
) -> Result<String> {
    let chosen = match service {
        ModelService::Brama { .. } => {
            let mut questions = serde_json::Map::new();
            questions.insert(
                key.to_string(),
                json!({"type": "choice", "instructions": instructions, "criteria": criteria}),
            );
            let payload = json!({"model": DECISION_ALIAS, "state": state, "questions": Value::Object(questions)});
            let answer = service.post("/v1/decisions", &payload, "the decision request failed")?;
            answer["answers"][key]["choice"]
                .as_str()
                .map(str::to_owned)
                .with_context(|| format!("Brama's decision answered no `{key}` choice: {answer}"))?
        }
        ModelService::Endpoint {
            model, provider, ..
        } => {
            let options = serde_json::to_string_pretty(criteria)?;
            let payload = json!({
                "model": model,
                "temperature": 0,
                "messages": [
                    {"role": "system", "content": format!(
                        "{instructions}\nChoose exactly one option, by its name, from these options and their meanings:\n{options}\nAnswer with only the JSON object {{\"choice\": \"<option name>\"}} and nothing else."
                    )},
                    {"role": "user", "content": serde_json::to_string_pretty(state)?}
                ]
            });
            let answer = service.post(
                "/v1/chat/completions",
                &payload,
                "the decision request failed",
            )?;
            let content = answer["choices"][0]["message"]["content"]
                .as_str()
                .with_context(|| format!("{provider} answered no message: {answer}"))?;
            let trimmed = content
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            let parsed: Value = serde_json::from_str(trimmed).with_context(|| {
                format!("{provider} answered the `{key}` choice with something that is not {{\"choice\": …}}: {content}")
            })?;
            parsed["choice"]
                .as_str()
                .map(str::to_owned)
                .with_context(|| format!("{provider} answered no `{key}` choice: {content}"))?
        }
    };
    if !criteria.contains_key(&chosen) {
        bail!(
            "{} chose `{chosen}` for `{key}`, which is not one of the options asked",
            service.provider()
        );
    }
    Ok(chosen)
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
