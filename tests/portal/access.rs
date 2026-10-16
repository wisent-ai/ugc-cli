//! A creator portal access lasts as long as the operator states: without
//! --days it never expires, with --days it expires after it was created, and a
//! validity that is not positive is refused and stores nothing.

#[path = "../support/journey.rs"]
mod support;

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use support::{Journey, journey, text};

fn accesses(journey: &mut Journey, creator: &str) -> Result<usize> {
    let listed = journey.accepted(&["standalone", "portal-list", "--creator", creator])?;
    listed
        .as_array()
        .map(Vec::len)
        .context("portal-list printed no list")
}

fn stated_validity(journey: &mut Journey) -> Result<()> {
    let creator = journey.accepted(&["creator", "add", "--name", "Portal Creator"])?;
    let creator = text(&creator, "/id")?.to_string();

    let open = journey.accepted(&["standalone", "portal-create", "--creator", &creator])?;
    ensure!(
        open.pointer("/access/expires_at") == Some(&Value::Null),
        "an access created without --days expires: {open}"
    );

    let dated = journey.accepted(&[
        "standalone",
        "portal-create",
        "--creator",
        &creator,
        "--days",
        "7",
    ])?;
    let expires = chrono::DateTime::parse_from_rfc3339(text(&dated, "/access/expires_at")?)?;
    let created = chrono::DateTime::parse_from_rfc3339(text(&dated, "/access/created_at")?)?;
    ensure!(
        expires > created,
        "an access created with --days expires at {expires}, not after its creation at {created}"
    );

    let before = accesses(journey, &creator)?;
    journey.refused(
        &[
            "standalone",
            "portal-create",
            "--creator",
            &creator,
            "--days",
            "0",
        ],
        "portal validity days must be positive",
    )?;
    let after = accesses(journey, &creator)?;
    ensure!(
        after == before,
        "a refused access was stored: {before} then {after}"
    );
    Ok(())
}

#[test]
fn a_portal_access_lasts_as_long_as_the_operator_states() -> Result<()> {
    journey("portal-validity", stated_validity)
}
