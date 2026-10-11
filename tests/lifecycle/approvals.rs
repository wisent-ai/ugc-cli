use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client as HttpClient;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use stado_database::{FleetDatabase, params, sync::Client};
use std::{env, fs, path::PathBuf, process::Command};
use uuid::Uuid;

struct Journey {
    binary: PathBuf,
    root: PathBuf,
    actor: String,
    commands: Vec<Value>,
    requests: Vec<Value>,
}
impl Journey {
    fn cli(&mut self, args: &[&str]) -> Result<Value> {
        let output = Command::new(&self.binary)
            .args(["--actor", &self.actor, "--asset-dir"])
            .arg(self.root.join("assets"))
            .arg("--json")
            .args(args)
            .output()?;
        self.commands.push(json!({"args": args, "status": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr)}));
        ensure!(
            output.status.success(),
            "CLI refused {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    }
    fn post(
        &mut self,
        http: &HttpClient,
        origin: &str,
        token: &str,
        path: &str,
        body: &Value,
        success: bool,
    ) -> Result<Value> {
        let response = http
            .post(format!("{origin}{path}"))
            .bearer_auth(token)
            .json(body)
            .send()?;
        let status = response.status();
        let text = response.text()?;
        self.requests
            .push(json!({"path": path, "status": status.as_u16(), "body": body, "response": text}));
        ensure!(status.is_success() == success, "HTTP {status}: {text}");
        let value: Value = serde_json::from_str(&text)?;
        if !success {
            ensure!(
                value["error"].is_string(),
                "refusal did not carry a cause: {value}"
            );
        }
        Ok(value)
    }
}

#[test]
#[ignore = "requires a real authenticated operator server sharing the isolated fleet database"]
fn operator_approvals_persist_and_refuse_invalid_transitions() -> Result<()> {
    let revision = env::var("UGC_TEST_SOURCE_REVISION")?;
    let expected_database = env::var("UGC_TEST_DATABASE_NAME")?;
    let origin = env::var("UGC_TEST_OPERATOR_ORIGIN")?;
    let token = fs::read_to_string(env::var("UGC_TEST_OPERATOR_TOKEN_FILE")?)?;
    ensure!(!token.trim().is_empty(), "operator token file is empty");
    let currency = env::var("UGC_TEST_CURRENCY")?;
    let service = env::var("UGC_TEST_SERVICE_TYPE")?;
    let evidence = fs::read(env::var("UGC_TEST_DEPLOYMENT_EVIDENCE")?)?;
    let head = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    ensure!(
        head.status.success() && String::from_utf8(head.stdout)?.trim() == revision,
        "candidate revision differs"
    );
    let clean = Command::new("git")
        .args(["status", "--porcelain"])
        .output()?;
    ensure!(
        clean.status.success() && clean.stdout.is_empty(),
        "candidate checkout must be clean"
    );
    let database = FleetDatabase::for_product("ugc-cli", "UGC_FLEET_HOME")?;
    let db = Client::connect(&database)?;
    let observed: String = db.query_row("SELECT current_database() AS name", params![], |row| {
        row.get("name")
    })?;
    ensure!(
        observed == expected_database,
        "database {observed} is not the isolated qualification database"
    );
    let actor = Uuid::new_v4().to_string();
    let root = PathBuf::from("target/real-tests/operator-approvals").join(&actor);
    fs::create_dir_all(root.join("assets"))?;
    fs::write(root.join("deployment-evidence"), evidence)?;
    let binary = fs::canonicalize(env!("CARGO_BIN_EXE_ugc-cli"))?;
    let binary_sha256 = hex::encode(Sha256::digest(fs::read(&binary)?));
    let mut journey = Journey {
        binary,
        root,
        actor,
        commands: Vec::new(),
        requests: Vec::new(),
    };
    let http = HttpClient::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut owned = Vec::<String>::new();
    let result: Result<()> = (|| {
        let name = journey.actor.clone();
        let campaign = journey.cli(&[
            "campaign",
            "create",
            "--name",
            &name,
            "--brand",
            &name,
            "--product",
            &name,
            "--currency",
            &currency,
        ])?;
        let campaign_id = campaign["id"]
            .as_str()
            .context("campaign id absent")?
            .to_owned();
        owned.push(campaign_id.clone());
        let brief = journey.cli(&[
            "brief",
            "add",
            "--campaign",
            &campaign_id,
            "--service-type",
            &service,
            "--creative-angle",
            &name,
        ])?;
        let brief_id = brief["id"].as_str().context("brief id absent")?.to_owned();
        owned.push(brief_id.clone());
        let creator = journey.cli(&["creator", "add", "--name", &name])?;
        let creator_id = creator["id"]
            .as_str()
            .context("creator id absent")?
            .to_owned();
        owned.push(creator_id.clone());
        let approve = format!("/api/briefs/{brief_id}/approve");
        journey.post(
            &http,
            &origin,
            &Uuid::new_v4().to_string(),
            &approve,
            &json!({}),
            false,
        )?;
        ensure!(
            journey.cli(&["brief", "show", &brief_id])? == brief,
            "unauthorized approval changed the brief"
        );
        let approved = journey.post(&http, &origin, token.trim(), &approve, &json!({}), true)?;
        ensure!(
            approved["status"] == "approved",
            "approval did not retain approved state"
        );
        ensure!(
            journey.cli(&["brief", "show", &brief_id])? == approved,
            "CLI did not read the HTTP-approved brief"
        );
        journey.post(&http, &origin, token.trim(), &approve, &json!({}), false)?;
        ensure!(
            journey.cli(&["brief", "show", &brief_id])? == approved,
            "repeated approval mutated the brief"
        );
        let verify = format!("/api/creators/{creator_id}/verify");
        journey.post(&http, &origin, token.trim(), &verify, &json!([]), false)?;
        ensure!(
            journey.cli(&["creator", "show", &creator_id])? == creator,
            "non-object verification changed the creator"
        );
        let evidence = json!({"qualification_run": journey.actor});
        let verified = journey.post(&http, &origin, token.trim(), &verify, &evidence, true)?;
        ensure!(
            verified["metadata"]["qualification_run"] == journey.actor,
            "verification evidence was lost"
        );
        ensure!(
            verified["metadata"]["verification_status"] == "verified",
            "creator verification state was not stored"
        );
        ensure!(
            journey.cli(&["creator", "show", &creator_id])? == verified,
            "CLI did not read the HTTP-verified creator"
        );
        Ok(())
    })();
    let mut cleanup_errors = Vec::new();
    for id in owned.into_iter().rev() {
        let cleanup: Result<()> = (|| {
            db.execute(
                "DELETE FROM audit_events WHERE aggregate_id=$1",
                params![id],
            )?;
            db.execute("DELETE FROM records WHERE id=$1", params![id])?;
            let remains: bool = db.query_row(
                "SELECT EXISTS(SELECT id FROM records WHERE id=$1) AS remains",
                params![id],
                |row| row.get("remains"),
            )?;
            ensure!(!remains, "owned record {id} remains");
            Ok(())
        })();
        if let Err(error) = cleanup {
            cleanup_errors.push(format!("{error:#}"));
        }
    }
    fs::write(
        journey.root.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "revision": revision, "binary_sha256": binary_sha256, "database": observed,
            "commands": journey.commands, "requests": journey.requests,
            "error": result.as_ref().err().map(|error| format!("{error:#}")), "cleanup_errors": cleanup_errors,
            "scope": "Actual operator HTTP writes and CLI/database readback; no browser or external verification claim"
        }))?,
    )?;
    ensure!(
        cleanup_errors.is_empty(),
        "cleanup failed: {cleanup_errors:?}"
    );
    result
}
