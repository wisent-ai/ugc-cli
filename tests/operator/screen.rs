//! The operator screen's writes: `standalone serve` on the journey's own
//! database creates, edits, approves, archives, cancels and removes through
//! its HTTP routes, and the CLI reads back exactly what each route wrote.
//! Without the operator token a write is refused and nothing is stored.
//!
//! The server's request limits are the caller's: `UGC_JOURNEY_MAX_HEADER_LINE_BYTES`,
//! `UGC_JOURNEY_MAX_HEADER_COUNT` and `UGC_JOURNEY_MAX_BODY_BYTES`.

#[path = "../support/journey.rs"]
mod support;

use anyhow::{Context, Result, bail, ensure};
use reqwest::{Method, blocking::Client};
use serde_json::{Value, json};
use std::{
    env,
    io::{BufRead, BufReader},
    net::{Ipv4Addr, TcpListener},
    process::{Child, Stdio},
};
use support::{Journey, journey, text};
use uuid::Uuid;

/// The token source the server reads, set only in the server's environment.
const TOKEN_VARIABLE: &str = "UGC_JOURNEY_OPERATOR_TOKEN";

struct Server {
    child: Child,
    origin: String,
    token: String,
    http: Client,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill() {
            eprintln!("the operator server could not be stopped: {error}");
        }
    }
}

fn stated(variable: &str) -> Result<String> {
    env::var(variable).with_context(|| {
        format!("{variable} must state the operator server's limit for this journey")
    })
}

/// Start the screen on a free loopback port and return once it says it is
/// ready.
fn serve(journey: &Journey) -> Result<Server> {
    let header_line = stated("UGC_JOURNEY_MAX_HEADER_LINE_BYTES")?;
    let header_count = stated("UGC_JOURNEY_MAX_HEADER_COUNT")?;
    let body = stated("UGC_JOURNEY_MAX_BODY_BYTES")?;
    // Port 0 asks the system for a free port, which the server then binds.
    let free = TcpListener::bind(format!("{}:0", Ipv4Addr::LOCALHOST))?.local_addr()?;
    let bind = free.to_string();
    let token = Uuid::new_v4().to_string();
    let mut child = journey
        .command(&[
            "standalone",
            "serve",
            "--bind",
            &bind,
            "--operator-token-source",
            &format!("env:{TOKEN_VARIABLE}"),
            "--max-header-line-bytes",
            &header_line,
            "--max-header-count",
            &header_count,
            "--max-body-bytes",
            &body,
        ])
        .env(TOKEN_VARIABLE, &token)
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().context("the server has no stderr")?;
    let mut said = Vec::new();
    for line in BufReader::new(stderr).lines() {
        let line = line?;
        let ready = line.contains("standalone UGC server ready on");
        said.push(line);
        if ready {
            return Ok(Server {
                child,
                origin: format!("http://{bind}"),
                token,
                http: Client::new(),
            });
        }
    }
    bail!("the operator server stopped before it was ready: {said:?}")
}

impl Server {
    fn call(
        &self,
        journey: &mut Journey,
        method: Method,
        path: &str,
        body: &Value,
        token: &str,
    ) -> Result<(bool, Value)> {
        let response = self
            .http
            .request(method.clone(), format!("{}{path}", self.origin))
            .bearer_auth(token)
            .json(body)
            .send()?;
        let status = response.status();
        let answer: Value = serde_json::from_str(&response.text()?)?;
        journey.commands.push(json!({
            "http": format!("{method} {path}"),
            "body": body,
            "status": status.as_u16(),
            "response": answer,
        }));
        Ok((status.is_success(), answer))
    }

    fn accepted(
        &self,
        journey: &mut Journey,
        method: Method,
        path: &str,
        body: Value,
    ) -> Result<Value> {
        let (success, answer) = self.call(journey, method.clone(), path, &body, &self.token)?;
        ensure!(success, "{method} {path} was refused: {answer}");
        Ok(answer)
    }

    fn refused(
        &self,
        journey: &mut Journey,
        method: Method,
        path: &str,
        body: Value,
        token: &str,
        cause: &str,
    ) -> Result<()> {
        let (success, answer) = self.call(journey, method.clone(), path, &body, token)?;
        ensure!(!success, "{method} {path} was accepted: {answer}");
        ensure!(
            answer["error"]
                .as_str()
                .is_some_and(|error| error.contains(cause)),
            "{method} {path} was refused without naming `{cause}`: {answer}"
        );
        Ok(())
    }
}

fn operator_screen(journey: &mut Journey) -> Result<()> {
    let server = serve(journey)?;
    let campaign_input = json!({
        "name": "Screen Campaign",
        "brand": "Example Brand",
        "product": "Example Lamp",
        "currency": "USD"
    });
    let stranger = Uuid::new_v4().to_string();
    server.refused(
        journey,
        Method::POST,
        "/api/campaigns",
        campaign_input.clone(),
        &stranger,
        "operator authorization failed",
    )?;
    ensure!(
        journey
            .accepted(&["campaign", "list"])?
            .as_array()
            .is_some_and(Vec::is_empty),
        "a refused request stored a campaign"
    );

    let campaign = server.accepted(journey, Method::POST, "/api/campaigns", campaign_input)?;
    let campaign_id = text(&campaign, "/id")?.to_string();
    ensure!(
        journey.accepted(&["campaign", "show", &campaign_id])? == campaign,
        "the CLI reads another campaign than the screen created"
    );
    let path = format!("/api/campaigns/{campaign_id}");
    server.refused(
        journey,
        Method::PATCH,
        &path,
        json!({}),
        &server.token,
        "campaign edit needs at least one of",
    )?;
    let renamed = server.accepted(
        journey,
        Method::PATCH,
        &path,
        json!({"name": "Renamed Campaign"}),
    )?;
    ensure!(
        text(&renamed, "/name")? == "Renamed Campaign",
        "the edit did not rename: {renamed}"
    );
    ensure!(
        journey.accepted(&["campaign", "show", &campaign_id])? == renamed,
        "the CLI does not read the edit"
    );

    let brief = server.accepted(
        journey,
        Method::POST,
        "/api/briefs",
        json!({"campaign_id": campaign_id, "service_type": "video", "creative_angle": "a lamp at dusk"}),
    )?;
    let brief_id = text(&brief, "/id")?.to_string();
    let approved = server.accepted(
        journey,
        Method::POST,
        &format!("/api/briefs/{brief_id}/approve"),
        json!({}),
    )?;
    ensure!(
        text(&approved, "/status")? == "approved",
        "the brief was not approved: {approved}"
    );
    let archived = server.accepted(
        journey,
        Method::POST,
        &format!("/api/briefs/{brief_id}/archive"),
        json!({}),
    )?;
    ensure!(
        text(&archived, "/status")? == "archived",
        "the brief was not archived: {archived}"
    );
    ensure!(
        journey.accepted(&["brief", "show", &brief_id])? == archived,
        "the CLI does not read the archived brief"
    );

    let creator = server.accepted(
        journey,
        Method::POST,
        "/api/creators",
        json!({"display_name": "Screen Creator", "email": "screen@studio.example"}),
    )?;
    let creator_id = text(&creator, "/id")?.to_string();
    let creator_path = format!("/api/creators/{creator_id}");
    let edited = server.accepted(
        journey,
        Method::PATCH,
        &creator_path,
        json!({"niches": ["lighting"]}),
    )?;
    ensure!(
        edited["niches"] == json!(["lighting"]),
        "the creator edit was not kept: {edited}"
    );
    ensure!(
        journey.accepted(&["creator", "show", &creator_id])? == edited,
        "the CLI does not read the creator edit"
    );
    server.accepted(journey, Method::DELETE, &creator_path, json!({}))?;
    journey.refused(
        &["creator", "show", &creator_id],
        &format!("creator not found: {creator_id}"),
    )?;

    let cancelled = server.accepted(
        journey,
        Method::POST,
        &format!("{path}/status"),
        json!({"status": "cancelled"}),
    )?;
    ensure!(
        text(&cancelled, "/status")? == "cancelled",
        "the campaign was not cancelled: {cancelled}"
    );
    ensure!(
        journey.accepted(&["campaign", "show", &campaign_id])? == cancelled,
        "the CLI does not read the cancellation"
    );
    Ok(())
}

#[test]
fn operator_screen_writes_reach_the_ledger_the_cli_reads() -> Result<()> {
    journey("operator-screen", operator_screen)
}
