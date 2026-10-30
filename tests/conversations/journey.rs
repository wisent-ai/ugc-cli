//! Conversations on a machine without Brama: the real `ugc-cli` binary on a
//! SQLite fleet database named in its own environment, asking the real Stado
//! `UGC_TEST_STADO` (in the journey's own home) which model service answers.

#[path = "../support/journey.rs"]
mod support;

use anyhow::{Context, Result, ensure};
use serde_json::json;
use support::{Journey, journey, text};

fn message_count(journey: &mut Journey, conversation: &str) -> Result<usize> {
    let messages = journey.accepted(&["standalone", "conversation-messages", conversation])?;
    messages
        .as_array()
        .map(Vec::len)
        .context("conversation-messages printed no list")
}

/// The Stado under test (`UGC_TEST_STADO`), run with the journey's own HOME,
/// with `input` on its standard input; kept in the journey's report.
fn stado(journey: &mut Journey, args: &[&str], input: Option<&str>) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut command = Command::new(journey.root.join("bin").join("stado"));
    command.args(args);
    journey.isolate(&mut command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .context("set UGC_TEST_STADO to the real stado executable")?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .context("stado's stdin is piped")?
            .write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    journey.commands.push(json!({
        "program": "stado",
        "args": args,
        "status": output.status.code(),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
    }));
    ensure!(
        output.status.success(),
        "stado {} refused: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn stated_intent(journey: &mut Journey) -> Result<()> {
    let creator = journey.accepted(&["creator", "add", "--name", "Stated Intent Creator"])?;
    let creator = text(&creator, "/id")?.to_string();
    let opened = journey.accepted(&[
        "standalone",
        "conversation-create",
        "--creator",
        &creator,
        "--currency",
        "USD",
        "--message",
        "Would you make a video for us?",
    ])?;
    let conversation = text(&opened, "/conversation/id")?.to_string();
    let before = message_count(journey, &conversation)?;

    let received = journey.accepted(&[
        "standalone",
        "conversation-receive",
        &conversation,
        "--body",
        "Sounds good, tell me more.",
        "--channel",
        "email",
        "--intent",
        "interested",
    ])?;
    ensure!(text(&received, "/inbound/intent")? == "interested");
    ensure!(text(&received, "/conversation/status")? == "interested");
    ensure!(text(&received, "/conversation/stage")? == "qualification");
    let after = message_count(journey, &conversation)?;
    ensure!(
        after > before,
        "the stated message was not stored: {before} then {after}"
    );
    let reply = text(&received, "/automated_reply/body")?;
    ensure!(
        reply.contains("No offer is recorded for this campaign yet"),
        "the reply to a conversation with no offer states one: {reply}"
    );

    let offered = journey.accepted(&[
        "standalone",
        "conversation-create",
        "--creator",
        &creator,
        "--currency",
        "USD",
        "--offer-minor",
        "25000",
        "--message",
        "Would you make a second video for us?",
    ])?;
    let offered = text(&offered, "/conversation/id")?.to_string();
    let priced = journey.accepted(&[
        "standalone",
        "conversation-receive",
        &offered,
        "--body",
        "What does it pay?",
        "--channel",
        "email",
        "--intent",
        "pricing",
    ])?;
    let reply = text(&priced, "/automated_reply/body")?;
    ensure!(
        reply.contains("Our recorded offer is 25000 USD in minor units"),
        "the reply does not state the recorded offer: {reply}"
    );

    journey.refused(
        &[
            "standalone",
            "conversation-receive",
            &conversation,
            "--body",
            "Maybe.",
            "--channel",
            "email",
            "--intent",
            "maybe",
        ],
        "`maybe` is not an intent; state one of opt_out, declined, accepted",
    )?;
    journey.refused(
        &[
            "standalone",
            "conversation-receive",
            &conversation,
            "--body",
            "Maybe.",
            "--channel",
            "email",
        ],
        "BRAMA_URL is not set and no inference alternative answers ugc: brama at ",
    )?;
    let refused_after = message_count(journey, &conversation)?;
    ensure!(
        refused_after == after,
        "a refused message was stored: {after} then {refused_after}"
    );

    journey.refused(
        &[
            "standalone",
            "ledger-fund",
            "--assignment",
            "none",
            "--amount-minor",
            "100",
            "--currency",
            "USD",
        ],
        "--idempotency-key",
    )
}

/// An invitation names the campaign create_conversation read, and a campaign
/// that cannot be read refuses the conversation instead of a generic invitation.
fn campaign_invitation(journey: &mut Journey) -> Result<()> {
    let creator = journey.accepted(&["creator", "add", "--name", "Invited Creator"])?;
    let creator = text(&creator, "/id")?.to_string();
    let campaign = journey.accepted(&[
        "campaign",
        "create",
        "--name",
        "Autumn Launch",
        "--brand",
        "Example Brand",
        "--product",
        "Example Kettle",
        "--currency",
        "USD",
    ])?;
    let campaign = text(&campaign, "/id")?.to_string();
    let opened = journey.accepted(&[
        "standalone",
        "conversation-create",
        "--creator",
        &creator,
        "--campaign",
        &campaign,
        "--currency",
        "USD",
    ])?;
    let invitation = text(&opened, "/message/body")?;
    ensure!(
        invitation.contains("'Autumn Launch'") && invitation.contains("Example Kettle"),
        "the invitation does not name the campaign it was read from: {invitation}"
    );
    let conversation = text(&opened, "/conversation/id")?.to_string();
    let stored = journey.accepted(&["standalone", "conversation-messages", &conversation])?;
    ensure!(
        stored
            .as_array()
            .is_some_and(|messages| messages.iter().any(|message| message["body"] == invitation)),
        "the stored conversation does not hold the invitation: {stored}"
    );

    let listed = |journey: &mut Journey| -> Result<usize> {
        let conversations =
            journey.accepted(&["standalone", "conversation-list", "--creator", &creator])?;
        conversations
            .as_array()
            .map(Vec::len)
            .context("conversation-list printed no list")
    };
    let before = listed(journey)?;
    journey.refused(
        &[
            "standalone",
            "conversation-create",
            "--creator",
            &creator,
            "--campaign",
            "no-such-campaign",
            "--currency",
            "USD",
        ],
        "campaign not found: no-such-campaign",
    )?;
    let after = listed(journey)?;
    ensure!(
        after == before,
        "a refused conversation was stored: {before} then {after}"
    );
    Ok(())
}

#[test]
fn an_operator_stated_intent_needs_no_brama() -> Result<()> {
    journey("stated-intent", stated_intent)
}

#[test]
fn an_invitation_names_the_campaign_it_was_read_from() -> Result<()> {
    journey("campaign-invitation", campaign_invitation)
}

/// With no Brama placed anywhere, an inbound message's intent is judged by
/// the real OpenAI-compatible endpoint (`UGC_TEST_ENDPOINT`, serving
/// `UGC_TEST_ENDPOINT_MODEL`, its key `UGC_TEST_ENDPOINT_KEY`) declared in
/// Brama's place through the real Stado `UGC_TEST_STADO`; Stado records that
/// the endpoint served ugc. Before it is declared, the message is refused
/// naming every alternative and nothing is stored.
fn intent_judged_in_bramas_place(journey: &mut Journey) -> Result<()> {
    let endpoint = std::env::var("UGC_TEST_ENDPOINT")
        .context("set UGC_TEST_ENDPOINT to a real OpenAI-compatible base url")?;
    let model = std::env::var("UGC_TEST_ENDPOINT_MODEL")
        .context("set UGC_TEST_ENDPOINT_MODEL to the model that endpoint serves")?;
    let key = std::env::var("UGC_TEST_ENDPOINT_KEY")
        .context("set UGC_TEST_ENDPOINT_KEY to that endpoint's key")?;
    stado(journey, &["config", "init"], None)?;
    let creator = journey.accepted(&["creator", "add", "--name", "Judged Intent Creator"])?;
    let creator = text(&creator, "/id")?.to_string();
    let opened = journey.accepted(&[
        "standalone",
        "conversation-create",
        "--creator",
        &creator,
        "--currency",
        "USD",
        "--message",
        "Would you make a video for us?",
    ])?;
    let conversation = text(&opened, "/conversation/id")?.to_string();
    let before = message_count(journey, &conversation)?;
    let receive = [
        "standalone",
        "conversation-receive",
        &conversation,
        "--body",
        "No thanks, I am not interested and please do not contact me again.",
        "--channel",
        "email",
    ];
    journey.refused(
        &receive,
        "model-endpoint at capabilities.inference.endpoint.url",
    )?;
    ensure!(
        message_count(journey, &conversation)? == before,
        "a message nobody judged was stored"
    );

    let store = journey.root.join("credentials.json");
    let store = format!("file://{}", store.display());
    stado(
        journey,
        &["config", "set", "credentials.store", &store],
        None,
    )?;
    stado(
        journey,
        &["credentials", "put", "--role", "ugc-journey-endpoint"],
        Some(&format!("{{\"token\":\"{key}\"}}")),
    )?;
    for (field, value) in [
        ("capabilities.inference.endpoint.url", endpoint.as_str()),
        ("capabilities.inference.endpoint.model", model.as_str()),
        (
            "capabilities.inference.endpoint.key_role",
            "ugc-journey-endpoint",
        ),
    ] {
        stado(journey, &["config", "set", field, value], None)?;
    }
    let received = journey.accepted(&receive)?;
    let intent = text(&received, "/inbound/intent")?;
    ensure!(
        ["opt_out", "declined"].contains(&intent),
        "the endpoint judged a refusal to continue as `{intent}`"
    );
    ensure!(
        message_count(journey, &conversation)? > before,
        "the judged message was not stored"
    );
    let listed: serde_json::Value = serde_json::from_str(&stado(
        journey,
        &["capabilities", "inference", "--json"],
        None,
    )?)?;
    ensure!(
        listed["alternatives"][0]["served"]["ugc"]["provider"] == "model-endpoint",
        "Stado does not record model-endpoint serving ugc: {listed}"
    );
    Ok(())
}

#[test]
fn an_intent_is_judged_by_the_endpoint_stado_resolves_in_bramas_place() -> Result<()> {
    journey("judged-intent", intent_judged_in_bramas_place)
}
