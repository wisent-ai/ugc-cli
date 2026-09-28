//! The ugc-cli ledger through the real binary against the real fleet
//! database `ugc-cli`, resolved through Stado and Skarbiec on this host. Each
//! run writes one campaign under a name of its own and reads it back; nothing
//! another run or the operator wrote is read or changed.

use std::process::Command;

use serde_json::Value;

fn ugc(arguments: &[&str]) -> (bool, Value, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ugc-cli"))
        .args(arguments)
        .output()
        .expect("the ugc-cli binary starts");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.success(), serde_json::from_str(&stdout).unwrap_or(Value::Null), stderr)
}

fn succeed(arguments: &[&str]) -> Value {
    let (ok, value, stderr) = ugc(arguments);
    assert!(ok, "ugc-cli {arguments:?} failed: {stderr}");
    value
}

#[test]
fn a_campaign_is_written_and_read_back_from_the_fleet_ledger() {
    let name = format!("ledger journey {}", uuid::Uuid::new_v4());
    let created = succeed(&[
        "campaign", "create", "--name", &name, "--brand", "Journey brand", "--product", "Journey product",
        "--markets", "US", "--languages", "en", "--channels", "short-video", "--currency", "USD",
    ]);
    let id = created["id"].as_str().expect("the campaign has an id").to_owned();

    let shown = succeed(&["campaign", "show", &id]);
    assert_eq!(shown["name"], name.as_str(), "the fleet ledger reads the campaign back by id");

    let listed = succeed(&["campaign", "list"]);
    assert!(
        listed.as_array().is_some_and(|campaigns| campaigns.iter().any(|campaign| campaign["id"] == id.as_str())),
        "the campaign is in the list the fleet ledger answers"
    );

    let audit = succeed(&["audit", "--kind", "campaign", "--id", &id]);
    assert!(
        audit.as_array().is_some_and(|entries| !entries.is_empty()),
        "creating the campaign left an audit entry in the fleet ledger"
    );

    let missing = format!("absent-{}", uuid::Uuid::new_v4());
    let (ok, _, stderr) = ugc(&["campaign", "show", &missing]);
    assert!(!ok, "a campaign the ledger does not hold is refused");
    assert!(stderr.contains(&format!("campaign not found: {missing}")), "{stderr}");
}
