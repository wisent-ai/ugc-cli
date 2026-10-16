//! Conversations on a machine without Brama or Stado: the real `ugc-cli`
//! binary on a SQLite fleet database named in its own environment.

#[path = "../support/journey.rs"]
mod support;

use anyhow::{Context, Result, ensure};
use support::{Journey, journey, text};

fn message_count(journey: &mut Journey, conversation: &str) -> Result<usize> {
    let messages = journey.accepted(&["standalone", "conversation-messages", conversation])?;
    messages
        .as_array()
        .map(Vec::len)
        .context("conversation-messages printed no list")
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
        "BRAMA_URL is not set",
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
