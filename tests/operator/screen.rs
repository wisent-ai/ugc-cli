//! The operator screen's writes: `standalone serve` on the journey's own
//! database creates, edits, approves, archives, cancels and removes through
//! its HTTP routes, and the CLI reads back exactly what each route wrote.
//! Without the operator token a write is refused and nothing is stored; a
//! conversation without its currency or a message without its channel is
//! refused naming the missing field.

#[path = "../support/operator_server.rs"]
mod operator_server;
#[path = "../support/journey.rs"]
mod support;

use anyhow::{Context, Result, ensure};
use operator_server::{Server, serve};
use reqwest::Method;
use serde_json::json;
use support::{Journey, journey, text};
use uuid::Uuid;

fn campaign_and_brief(server: &Server, journey: &mut Journey) -> Result<()> {
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

fn creator(server: &Server, journey: &mut Journey) -> Result<()> {
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
    )
}

fn conversation(server: &Server, journey: &mut Journey) -> Result<()> {
    let talker = journey.accepted(&["creator", "add", "--name", "Screen Talker"])?;
    let talker_id = text(&talker, "/id")?.to_string();
    let opening = json!({"creator_id": talker_id, "initial_message": "Would you film a lamp?"});
    server.refused(
        journey,
        Method::POST,
        "/api/conversations",
        opening.clone(),
        &server.token,
        "missing field `currency`",
    )?;
    let mut stated = opening;
    stated["currency"] = json!("EUR");
    let opened = server.accepted(journey, Method::POST, "/api/conversations", stated)?;
    let conversation_id = text(&opened, "/conversation/id")?.to_string();
    ensure!(
        text(&opened, "/conversation/currency")? == "EUR",
        "the stated currency was not kept: {opened}"
    );
    let messages = format!("/api/conversations/{conversation_id}/messages");
    server.refused(
        journey,
        Method::POST,
        &messages,
        json!({"body": "Here is the brief"}),
        &server.token,
        "missing field `channel`",
    )?;
    server.accepted(
        journey,
        Method::POST,
        &messages,
        json!({"body": "Here is the brief", "channel": "email"}),
    )?;
    let read = journey.accepted(&["standalone", "conversation-messages", &conversation_id])?;
    let sent = read
        .as_array()
        .context("conversation-messages printed no array")?
        .iter()
        .find(|message| message["body"] == "Here is the brief")
        .with_context(|| format!("the CLI does not read the sent message: {read}"))?;
    ensure!(
        sent["channel"] == "email",
        "the stated channel was not kept: {sent}"
    );
    Ok(())
}

fn operator_screen(journey: &mut Journey) -> Result<()> {
    let server = serve(journey)?;
    campaign_and_brief(&server, journey)?;
    creator(&server, journey)?;
    conversation(&server, journey)
}

#[test]
fn operator_screen_writes_reach_the_ledger_the_cli_reads() -> Result<()> {
    journey("operator-screen", operator_screen)
}
