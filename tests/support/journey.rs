//! The harness every ugc-cli journey runs through: the built binary on a
//! SQLite fleet database the journey creates, named by `UGC_CLI_DATABASE_URL`,
//! with no Brama setting in its environment, and a report of every command.

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
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

pub struct Journey {
    pub binary: PathBuf,
    pub root: PathBuf,
    pub database_url: String,
    pub commands: Vec<Value>,
}

impl Journey {
    pub fn run(&mut self, args: &[&str]) -> Result<Output> {
        let mut command = Command::new(&self.binary);
        command
            .args(["--actor", "ugc-cli-journey", "--asset-dir"])
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

    pub fn accepted(&mut self, args: &[&str]) -> Result<Value> {
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

    pub fn refused(&mut self, args: &[&str], cause: &str) -> Result<()> {
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
}

pub fn text<'a>(value: &'a Value, pointer: &str) -> Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .with_context(|| format!("{pointer} is missing from {value}"))
}

fn create_database(root: &Path) -> Result<String> {
    let file = root.join("ugc-cli.sqlite");
    fs::File::create(&file)?;
    Ok(format!("sqlite://{}", fs::canonicalize(&file)?.display()))
}

/// Run `steps` on a fresh SQLite fleet database under
/// target/real-tests/`name` and keep every command in its report.
pub fn journey(name: &str, steps: fn(&mut Journey) -> Result<()>) -> Result<()> {
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
