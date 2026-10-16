//! Onboarding's first-success screen on a ledger whose stamped campaign can no
//! longer be read: the walk stops at awaiting_campaign and says why.

#[path = "../support/journey.rs"]
mod support;

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use stado_database::{FleetDatabase, params, sync::Client};
use support::{Journey, journey, text};

fn unreadable_campaign(journey: &mut Journey) -> Result<()> {
    let campaign = journey.accepted(&[
        "campaign",
        "create",
        "--name",
        "Stamped Campaign",
        "--brand",
        "Example Brand",
        "--product",
        "Example Lamp",
        "--currency",
        "USD",
    ])?;
    let campaign = text(&campaign, "/id")?.to_string();

    // This test binary holds this one journey, so naming its database in the
    // process environment reaches no other test.
    unsafe { std::env::set_var("UGC_CLI_DATABASE_URL", &journey.database_url) };
    let database = FleetDatabase::for_product("ugc-cli", "UGC_FLEET_HOME")?;
    Client::connect(&database)?.execute(
        "DELETE FROM records WHERE kind='campaign' AND id=$1",
        params![campaign],
    )?;
    journey.refused(
        &["campaign", "show", &campaign],
        &format!("campaign not found: {campaign}"),
    )?;

    let walked = journey.accepted(&["onboarding", "--yes"])?;
    ensure!(
        text(&walked, "/status")? == "awaiting_campaign",
        "onboarding did not stop at awaiting_campaign: {walked}"
    );
    let expected = format!(
        "The recorded campaign {campaign} can no longer be read out of this ledger: campaign not found: {campaign}"
    );
    let notes: Vec<&str> = walked
        .get("steps")
        .and_then(Value::as_array)
        .context("onboarding printed no steps")?
        .iter()
        .filter_map(|step| step.get("notes").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    ensure!(
        notes.contains(&expected.as_str()),
        "onboarding did not say why the campaign is unreadable: {notes:?}"
    );
    Ok(())
}

#[test]
fn onboarding_says_why_the_stamped_campaign_is_unreadable() -> Result<()> {
    journey("onboarding-unreadable-campaign", unreadable_campaign)
}
