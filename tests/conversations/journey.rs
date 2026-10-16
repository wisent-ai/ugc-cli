//! An operator-stated intent on a machine without Brama or Stado: the real
//! `ugc-cli` binary on a SQLite fleet database named in its own environment.

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use uuid::Uuid;

/// Every setting the Brama judgement reads; none may reach the binary.
const BRAMA_SETTINGS: &[&str] = &[
    "BRAMA_URL",
    "UGC_BRAMA_AGENT_ID",
    "UGC_BRAMA_SIGNING_SECRET_SOURCE",
];

struct Journey {
    binary: PathBuf,
    root: PathBuf,
    database_url: String,
    commands: Vec<Value>,
}

impl Journey {
    fn run(&mut self, args: &[&str]) -> Result<Output> {
        let mut command = Command::new(&self.binary);
        command
            .args(["--actor", "stated-intent-journey", "--asset-dir"])
            .arg(self.root.join("assets"))
            .arg("--json")
            .args(args)
            .env("UGC_CLI_DATABASE_URL", &self.database_url)
            .env("UGC_FLEET_HOME", &self.root);
        for setting in BRAMA_SETTINGS {
            command.env_remove(setting);
        }
        let output = command.output()?;
        self.commands.push(json!({
            "args": args,
            "status": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }));
        Ok(output)
    }

    fn accepted(&mut self, args: &[&str]) -> Result<Value> {
        let output = self.run(args)?;
        ensure!(
            output.status.success(),
            "ugc-cli refused {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("ugc-cli {} printed no JSON", args.join(" ")))
    }

    fn refused(&mut self, args: &[&str], cause: &str) -> Result<()> {
        let output = self.run(args)?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        ensure!(
            !output.status.success(),
            "ugc-cli accepted {}",
            args.join(" ")
        );
        ensure!(
            stderr.contains(cause),
            "ugc-cli refused {} without naming `{cause}`: {stderr}",
            args.join(" ")
        );
        Ok(())
    }

    fn message_count(&mut self, conversation: &str) -> Result<usize> {
        let messages = self.accepted(&["standalone", "conversation-messages", conversation])?;
        messages
            .as_array()
            .map(Vec::len)
            .context("conversation-messages printed no list")
    }
}

fn text<'a>(value: &'a Value, pointer: &str) -> Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .with_context(|| format!("{pointer} is missing from {value}"))
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
    let before = journey.message_count(&conversation)?;

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
    let after = journey.message_count(&conversation)?;
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
    let refused_after = journey.message_count(&conversation)?;
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

fn create_database(root: &Path) -> Result<String> {
    let file = root.join("ugc-cli.sqlite");
    fs::File::create(&file)?;
    Ok(format!("sqlite://{}", fs::canonicalize(&file)?.display()))
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

/// Run `steps` on a fresh SQLite fleet database under
/// target/real-tests/`name` and keep every command in its report.
fn journey(name: &str, steps: fn(&mut Journey) -> Result<()>) -> Result<()> {
    let revision = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    let root = PathBuf::from("target/real-tests")
        .join(name)
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(root.join("assets"))?;
    let root = fs::canonicalize(root)?;
    let binary = fs::canonicalize(env!("CARGO_BIN_EXE_ugc-cli"))?;
    let mut journey = Journey {
        binary: binary.clone(),
        database_url: create_database(&root)?,
        root: root.clone(),
        commands: Vec::new(),
    };
    let result = steps(&mut journey);
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "source_revision": String::from_utf8_lossy(&revision.stdout).trim(),
            "binary": binary,
            "binary_sha256": hex::encode(Sha256::digest(fs::read(&binary)?)),
            "database_url": journey.database_url,
            "commands": journey.commands,
            "error": result.as_ref().err().map(|error| format!("{error:#}")),
        }))?,
    )?;
    result
}

#[test]
fn an_operator_stated_intent_needs_no_brama() -> Result<()> {
    journey("stated-intent", stated_intent)
}

#[test]
fn an_invitation_names_the_campaign_it_was_read_from() -> Result<()> {
    journey("campaign-invitation", campaign_invitation)
}
