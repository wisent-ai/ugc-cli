//! Creator import matches creators by email, whatever its case, and stops
//! naming the record when the creator list it matches against cannot be read,
//! instead of creating every imported creator again.

#[path = "../support/journey.rs"]
mod support;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use stado_database::{FleetDatabase, params, sync::Client};
use std::{
    fs,
    path::{Path, PathBuf},
};
use support::{Journey, journey};

fn ids<'a>(imported: &'a Value, field: &str) -> Result<Vec<&'a str>> {
    imported
        .get(field)
        .and_then(Value::as_array)
        .with_context(|| format!("the import printed no {field} list: {imported}"))?
        .iter()
        .map(|id| id.as_str().context("an imported id is not text"))
        .collect()
}

fn seed_file(journey: &Journey, name: &str, seeds: &Value) -> Result<PathBuf> {
    let file = journey.root.join(name);
    fs::write(&file, serde_json::to_vec(seeds)?)?;
    Ok(file)
}

fn path(file: &Path) -> Result<&str> {
    file.to_str().context("the seed file path is not UTF-8")
}

fn creators(journey: &mut Journey) -> Result<Vec<Value>> {
    journey
        .accepted(&["creator", "list"])?
        .as_array()
        .cloned()
        .context("creator list printed no array")
}

fn creator_import(journey: &mut Journey) -> Result<()> {
    let identity = json!({"platform": "tiktok", "external_creator_id": "ada-studio"});
    let seeds = json!([
        {"display_name": "Ada", "email": "ada@studio.example", "identities": [identity]},
        {"display_name": "Bo", "email": "bo@studio.example"}
    ]);
    let first = seed_file(journey, "first.json", &seeds)?;
    let imported = journey.accepted(&["standalone", "import-creators", path(&first)?])?;
    let created = ids(&imported, "created")?;
    ensure!(
        Some(created.len()) == seeds.as_array().map(Vec::len)
            && ids(&imported, "existing")?.is_empty(),
        "the first import did not create every seed: {imported}"
    );
    let ada = created
        .first()
        .context("the import created no creator")?
        .to_string();
    let bo = created
        .last()
        .context("the import created no creator")?
        .to_string();

    let again = seed_file(
        journey,
        "again.json",
        &json!([{"display_name": "Ada Again", "email": "ADA@STUDIO.EXAMPLE", "identities": [identity]}]),
    )?;
    let repeated = journey.accepted(&["standalone", "import-creators", path(&again)?])?;
    ensure!(
        ids(&repeated, "created")?.is_empty() && ids(&repeated, "existing")? == [ada.as_str()],
        "the repeated email was not matched to its creator: {repeated}"
    );
    ensure!(
        creators(journey)?.len() == created.len(),
        "the repeated import changed the number of creators"
    );

    // This test binary holds this one journey, so naming its database in the
    // process environment reaches no other test.
    unsafe { std::env::set_var("UGC_CLI_DATABASE_URL", &journey.database_url) };
    let database = Client::connect(&FleetDatabase::for_product("ugc-cli", "UGC_FLEET_HOME")?)?;
    database.execute(
        "UPDATE records SET data='{' WHERE kind='creator' AND id=$1",
        params![bo],
    )?;
    let later = seed_file(
        journey,
        "later.json",
        &json!([{"display_name": "Cy", "email": "cy@studio.example"}]),
    )?;
    journey.refused(
        &["standalone", "import-creators", path(&later)?],
        &format!("creator {bo} cannot be read"),
    )?;
    let stored: i64 = database.query_row(
        "SELECT COUNT(*) AS stored FROM records WHERE kind='creator'",
        params![],
        |row| row.get("stored"),
    )?;
    ensure!(
        usize::try_from(stored)? == created.len(),
        "the refused import still stored a creator: {stored} creators"
    );
    Ok(())
}

#[test]
fn creator_import_matches_by_email_and_stops_on_an_unreadable_list() -> Result<()> {
    journey("creator-import", creator_import)
}
