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
/// The consumer ugc asks Stado's capabilities as.
const CONSUMER: &str = "ugc";
/// The command each model request is handed to when no Brama is named.
const STADO_CALL: [&str; 6] = ["capabilities", "call", "inference", "--consumer", CONSUMER, "--json"];

/// Which model service answers ugc, and how it is called.
pub enum ModelService {
    /// Brama at the base URL the operator names; every request it serves for
    /// a model is signed as the agent with its secret, a health read with
    /// nothing.
    Brama {
        base_url: String,
        signing: Option<(String, String)>,
    },
    /// Stado's inference alternatives: each request is handed to `stado
    /// capabilities call inference --consumer ugc`, which asks the first
    /// alternative in its selected order that answers it and presents what
    /// that alternative needs. ugc names no alternative and reads no key.
    Stado,
}

/// A setting read only when it is needed: Brama's agent id or signing secret
/// source, read only when Brama is the service that answers.
pub type Deferred<'a> = Box<dyn FnOnce() -> Result<String> + 'a>;

impl ModelService {
    /// The model service ugc calls. A base URL the operator names (`--base-url`
    /// or `BRAMA_URL`) is Brama, used as given, with its signing settings when
    /// `signing` is given (a health read signs nothing). Otherwise each
    /// request goes to Stado's inference alternatives.
    pub fn resolve(
        named: Option<String>,
        signing: Option<(Deferred<'_>, Deferred<'_>)>,
    ) -> Result<Self> {
        let Some(base_url) = named.filter(|url| !url.trim().is_empty()) else {
            return Ok(ModelService::Stado);
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
            ModelService::Stado => "stado",
        }
    }

    /// POST `payload` to Brama's `path`, signed over the body
    /// (`x-agent-signature` = HMAC-SHA256 of `agent:timestamp:sha256(body)`);
    /// through Stado, a chat completion handed to `stado capabilities call`.
    fn post(&self, path: &str, payload: &Value, failed: &str) -> Result<Value> {
        let (base_url, signing) = match self {
            ModelService::Brama { base_url, signing } => (base_url.trim_end_matches('/'), signing),
            ModelService::Stado => {
                if path != "/v1/chat/completions" {
                    bail!("{path} is Brama's own route; through Stado only chat completions are asked");
                }
                return call_stado(payload);
            }
        };
        let Some((agent_id, secret)) = signing else {
            bail!(
                "Brama answers only requests signed as an agent; set UGC_BRAMA_AGENT_ID and UGC_BRAMA_SIGNING_SECRET_SOURCE"
            )
        };
        let body = serde_json::to_vec(payload)?;
        let endpoint = format!("{base_url}{path}");
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
            .to_string();
        let body_hash = hex::encode(Sha256::digest(&body));
        let signed = format!("{agent_id}:{timestamp}:{body_hash}");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
        mac.update(signed.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());
        let response = Client::new()
            .post(&endpoint)
            .header("content-type", "application/json")
            .header("x-agent-id", agent_id)
            .header("x-agent-timestamp", &timestamp)
            .header("x-agent-signature", signature)
            .body(body)
            .send()
            .with_context(|| format!("{failed} at {endpoint}"))?;
        json_response(response, self.provider())
    }
}

/// The `Error:` line of a failed Stado command, else its last line.
fn stado_cause(said: &str) -> String {
    let lines: Vec<&str> = said.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    lines
        .iter()
        .rev()
        .find_map(|line| line.strip_prefix("Error: "))
        .or_else(|| lines.last().copied())
        .unwrap_or("it said nothing")
        .to_owned()
}

/// Hand one chat-completions `payload` to Stado's inference alternatives and
/// read the answer the one that answered gave. Refused naming the command
/// when Stado cannot run, and with Stado's own sentence (each alternative's
/// cause and the command that answers the kept request) when none answers.
fn call_stado(payload: &Value) -> Result<Value> {
    use std::io::Write;
    let operation = format!("stado {}", STADO_CALL.join(" "));
    let mut child = std::process::Command::new("stado")
        .args(STADO_CALL)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| {
            format!("BRAMA_URL is not set and `{operation}` could not run; set BRAMA_URL or install Stado")
        })?;
    child
        .stdin
        .take()
        .context("stado's standard input could not be opened")?
        .write_all(&serde_json::to_vec(payload)?)
        .with_context(|| format!("the request could not be handed to `{operation}`"))?;
    let output = child
        .wait_with_output()
        .with_context(|| format!("`{operation}` did not end"))?;
    if !output.status.success() {
        bail!(
            "BRAMA_URL is not set and no inference alternative answered `{operation}` ({}): {}",
            output.status,
            stado_cause(&String::from_utf8_lossy(&output.stderr))
        );
    }
    let mut printed: Value = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("`{operation}` printed an answer that is not JSON"))?;
    match printed.get_mut("answer").map(Value::take) {
        Some(answer) if !answer.is_null() => Ok(answer),
        _ => bail!("`{operation}` printed no answer: {printed}"),
    }
}

/// Whether the model service answers: Brama's `/health`, or, through Stado,
/// the order of its inference alternatives, read without asking any of them.
pub fn model_health(service: &ModelService) -> Result<Value> {
    let base_url = match service {
        ModelService::Brama { base_url, .. } => base_url.trim_end_matches('/'),
        ModelService::Stado => {
            let operation = "stado capabilities inference --json";
            let output = std::process::Command::new("stado")
                .args(["capabilities", "inference", "--json"])
                .stdin(std::process::Stdio::null())
                .output()
                .with_context(|| {
                    format!("BRAMA_URL is not set and `{operation}` could not run; set BRAMA_URL or install Stado")
                })?;
            if !output.status.success() {
                bail!(
                    "BRAMA_URL is not set and `{operation}` failed ({}): {}",
                    output.status,
                    stado_cause(&String::from_utf8_lossy(&output.stderr))
                );
            }
            let listed: Value = serde_json::from_slice(&output.stdout)
                .with_context(|| format!("`{operation}` printed no JSON"))?;
            let family = listed["alternatives"]
                .as_array()
                .and_then(|families| families.iter().find(|family| family["family"] == "inference"))
                .cloned()
                .with_context(|| format!("`{operation}` lists no inference alternatives: {listed}"))?;
            return Ok(json!({
                "provider": service.provider(),
                "called_through": format!("stado {}", STADO_CALL.join(" ")),
                "order": family["order"],
                "served": family["served"][CONSUMER],
            }));
        }
    };
    let response = Client::new()
        .get(format!("{base_url}/health"))
        .send()
        .with_context(|| format!("cannot reach brama at {base_url}/health"))?;
    let answer = json_response(response, service.provider())?;
    Ok(json!({
        "provider": service.provider(),
        "base_url": base_url,
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
    // Every model service is asked for the model the operator names.
    required("model", model)?;
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
/// The function a choice asked through Stado is answered by calling.
const CHOICE_FUNCTION: &str = "record_choice";

/// One typed choice about `state`: `question` with `criteria` (option name to
/// meaning). A named Brama answers it through `POST /v1/decisions`; through
/// Stado, which carries chat completions only, it is one chat completion that
/// must call `record_choice` with `{"choice": "<option>"}`, the option bound
/// by the function's schema. Either answer that is not one of the options, or
/// not that shape, is refused with what the service sent.
pub fn model_choose(
    service: &ModelService,
    model: &str,
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
        ModelService::Stado => {
            required("model", model)?;
            let options: Vec<&String> = criteria.keys().collect();
            let payload = json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": format!(
                        "{instructions}\nChoose exactly one option from these options and their meanings, and record it with {CHOICE_FUNCTION}:\n{}",
                        serde_json::to_string_pretty(criteria)?
                    )},
                    {"role": "user", "content": serde_json::to_string_pretty(state)?}
                ],
                "tools": [{"type": "function", "function": {
                    "name": CHOICE_FUNCTION,
                    "description": format!("The `{key}` choice."),
                    "strict": true,
                    "parameters": {
                        "type": "object",
                        "properties": {"choice": {"type": "string", "enum": options}},
                        "required": ["choice"],
                        "additionalProperties": false,
                    },
                }}],
                "tool_choice": {"type": "function", "function": {"name": CHOICE_FUNCTION}},
            });
            let answer = service.post(
                "/v1/chat/completions",
                &payload,
                "the decision request failed",
            )?;
            let call = &answer["choices"][0]["message"]["tool_calls"][0]["function"];
            if call["name"] != CHOICE_FUNCTION {
                bail!("the answer to the `{key}` choice did not call {CHOICE_FUNCTION}: {answer}");
            }
            let arguments = call["arguments"].as_str().with_context(|| {
                format!("{CHOICE_FUNCTION} was called with no arguments text: {answer}")
            })?;
            let parsed: Value = serde_json::from_str(arguments).with_context(|| {
                format!("{CHOICE_FUNCTION} was called with arguments that are not JSON: {arguments}")
            })?;
            parsed["choice"]
                .as_str()
                .map(str::to_owned)
                .with_context(|| format!("{CHOICE_FUNCTION} names no `choice`: {arguments}"))?
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
