//! Each ugc-cli option has one spelling: a removed second spelling is refused
//! by the parser and stores nothing.

#[path = "../support/journey.rs"]
mod support;

use anyhow::{Context, Result, ensure};
use support::{Journey, journey};

fn removed_spellings(journey: &mut Journey) -> Result<()> {
    for removed in ["--token-env", "--webhook-secret-env"] {
        journey.refused(
            &[
                "connection",
                "add",
                "--name",
                "spelling-check",
                "--provider",
                "example",
                removed,
                "EXAMPLE_SECRET",
            ],
            &format!("unexpected argument '{removed}'"),
        )?;
    }
    let connections = journey.accepted(&["connection", "list"])?;
    ensure!(
        connections
            .as_array()
            .context("connection list printed no list")?
            .is_empty(),
        "a refused connection was stored: {connections}"
    );
    Ok(())
}

#[test]
fn removed_option_spellings_are_refused() -> Result<()> {
    journey("removed-spellings", removed_spellings)
}
