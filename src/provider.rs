use std::env;

use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
use reqwest::blocking::{Client, Response};
use serde_json::{Value, json};
use sha2::Sha256;
use uuid::Uuid;

use crate::model::{Connection, ProviderEvent};

pub trait ProviderAdapter {
    fn health(&self) -> Result<Value>;
    fn publish_campaign(&self, payload: &Value) -> Result<Value>;
    fn send_message(&self, payload: &Value) -> Result<Value>;
    fn request_revision(&self, payload: &Value) -> Result<Value>;
    fn poll(&self, cursor: Option<&str>) -> Result<(Vec<ProviderEvent>, Option<String>)>;
    fn verify_webhook(&self, body: &[u8], signature: Option<&str>) -> Result<()>;
}

pub fn adapter(connection: &Connection) -> Result<Box<dyn ProviderAdapter>> {
    match connection.provider.as_str() {
        "manual" => Ok(Box::new(ManualAdapter)),
        "http" => Ok(Box::new(HttpAdapter::new(connection)?)),
        other => bail!("unsupported provider: {other}"),
    }
}

struct ManualAdapter;

impl ProviderAdapter for ManualAdapter {
    fn health(&self) -> Result<Value> {
        Ok(json!({"healthy": true, "provider": "manual", "mode": "assisted"}))
    }

    fn publish_campaign(&self, _payload: &Value) -> Result<Value> {
        let id = format!("manual:{}", Uuid::new_v4());
        Ok(json!({"external_campaign_id": id, "status": "manual_action_required"}))
    }

    fn send_message(&self, _payload: &Value) -> Result<Value> {
        Ok(json!({"status": "manual_action_required"}))
    }

    fn request_revision(&self, _payload: &Value) -> Result<Value> {
        Ok(json!({"status": "manual_action_required"}))
    }

    fn poll(&self, cursor: Option<&str>) -> Result<(Vec<ProviderEvent>, Option<String>)> {
        Ok((Vec::new(), cursor.map(str::to_owned)))
    }

    fn verify_webhook(&self, _body: &[u8], _signature: Option<&str>) -> Result<()> {
        bail!("manual provider does not accept webhooks")
    }
}

struct HttpAdapter {
    client: Client,
    base_url: String,
    token: Option<String>,
    webhook_secret: Option<String>,
}

impl HttpAdapter {
    fn new(connection: &Connection) -> Result<Self> {
        let base_url = connection
            .base_url
            .clone()
            .context("http connection has no base_url")?;
        let token = connection
            .token_env
            .as_deref()
            .map(read_secret)
            .transpose()?;
        let webhook_secret = connection
            .webhook_secret_env
            .as_deref()
            .map(read_secret)
            .transpose()?;
        Ok(Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').into(),
            token,
            webhook_secret,
        })
    }

    fn request(&self, method: &str, path: &str, payload: Option<&Value>) -> Result<Response> {
        let url = format!("{}{}", self.base_url, path);
        let mut request = match method {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            "PATCH" => self.client.patch(&url),
            other => bail!("unsupported HTTP method: {other}"),
        };
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = payload {
            request = request.json(body);
        }
        request
            .send()
            .with_context(|| format!("provider request failed: {method} {url}"))
    }

    fn json_response(response: Response) -> Result<Value> {
        let status = response.status();
        let text = response.text()?;
        if !status.is_success() {
            bail!("provider returned {status}: {text}");
        }
        if text.trim().is_empty() {
            return Ok(json!({"status": "accepted"}));
        }
        serde_json::from_str(&text)
            .with_context(|| format!("provider returned invalid JSON: {text}"))
    }
}

impl ProviderAdapter for HttpAdapter {
    fn health(&self) -> Result<Value> {
        Self::json_response(self.request("GET", "/health", None)?)
    }

    fn publish_campaign(&self, payload: &Value) -> Result<Value> {
        Self::json_response(self.request("POST", "/campaigns", Some(payload))?)
    }

    fn send_message(&self, payload: &Value) -> Result<Value> {
        Self::json_response(self.request("POST", "/messages", Some(payload))?)
    }

    fn request_revision(&self, payload: &Value) -> Result<Value> {
        Self::json_response(self.request("POST", "/revisions", Some(payload))?)
    }

    fn poll(&self, cursor: Option<&str>) -> Result<(Vec<ProviderEvent>, Option<String>)> {
        let path = match cursor {
            Some(value) => format!("/events?cursor={}", url_encode(value)),
            None => "/events".into(),
        };
        let body = Self::json_response(self.request("GET", &path, None)?)?;
        let events = body
            .get("events")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let events: Vec<ProviderEvent> =
            serde_json::from_value(events).context("provider events use an invalid schema")?;
        let next = body
            .get("next_cursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok((events, next))
    }

    fn verify_webhook(&self, body: &[u8], signature: Option<&str>) -> Result<()> {
        let secret = self
            .webhook_secret
            .as_deref()
            .context("webhook secret is not configured")?;
        let signature = signature.context("webhook signature is missing")?;
        let expected = hex::decode(signature.trim_start_matches("sha256="))
            .context("webhook signature is not valid hexadecimal")?;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
        mac.update(body);
        mac.verify_slice(&expected)
            .map_err(|_| anyhow::anyhow!("webhook signature mismatch"))
    }
}

fn read_secret(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("environment variable {name} is not set"))
}

fn url_encode(input: &str) -> String {
    input
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
