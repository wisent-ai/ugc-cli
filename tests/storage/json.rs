use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use stado_database::{FleetDatabase, params, sync::Client};
use std::{env, fs, path::PathBuf, process::Command};
use uuid::Uuid;

struct Run {
    binary: PathBuf,
    root: PathBuf,
    actor: String,
    commands: Vec<Value>,
}
impl Run {
    fn cli(&mut self, args: &[&str], success: bool) -> Result<Value> {
        let output = Command::new(&self.binary)
            .args(["--actor", &self.actor, "--asset-dir"])
            .arg(self.root.join("assets"))
            .arg("--json")
            .args(args)
            .output()
            .context("executing the candidate UGC CLI")?;
        self.commands.push(json!({"args": args, "status": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr)}));
        ensure!(
            output.status.success() == success,
            "UGC CLI returned {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        if success {
            Ok(serde_json::from_slice(&output.stdout)?)
        } else {
            ensure!(
                output.stdout.is_empty(),
                "refused read published a successful payload"
            );
            Ok(Value::String(String::from_utf8(output.stderr)?))
        }
    }
}

#[test]
#[ignore = "requires the actual isolated fleet database and a compiled candidate"]
fn corrupt_json_is_refused_without_rewriting_the_row() -> Result<()> {
    let revision = env::var("UGC_TEST_SOURCE_REVISION")?;
    let database_name = env::var("UGC_TEST_DATABASE_NAME")?;
    let head = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    ensure!(head.status.success(), "git could not read source identity");
    ensure!(
        String::from_utf8(head.stdout)?.trim() == revision,
        "source revision differs"
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
        observed == database_name,
        "selected database {observed} is not the declared isolated qualification database"
    );
    let actor = Uuid::new_v4().to_string();
    let root = PathBuf::from("target/real-tests/stored-json").join(&actor);
    fs::create_dir_all(root.join("assets"))?;
    let binary = fs::canonicalize(env!("CARGO_BIN_EXE_ugc-cli"))?;
    let binary_sha256 = hex::encode(Sha256::digest(fs::read(&binary)?));
    let mut run = Run {
        binary,
        root,
        actor,
        commands: Vec::new(),
    };
    let mut creator: Option<String> = None;
    let mut owned = Vec::<(&str, String)>::new();
    let result: Result<()> = (|| {
        let name = run.actor.clone();
        let created = run.cli(&["creator", "add", "--name", &name], true)?;
        let creator_id = created["id"]
            .as_str()
            .context("creator add did not return its id")?
            .to_owned();
        creator = Some(creator_id.clone());
        let payload = json!({"qualification": run.actor}).to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let outbox = Uuid::new_v4().to_string();
        db.execute("INSERT INTO outbox(id,kind,aggregate_type,aggregate_id,payload,status,available_at,created_at,updated_at) VALUES($1,'qualification','creator',$2,$3,'pending',$4,$4,$4)", params![outbox, creator_id, payload, now])?;
        owned.push(("outbox", outbox.clone()));
        let webhook = Uuid::new_v4().to_string();
        db.execute("INSERT INTO webhook_events(id,connection_id,provider_event_id,event_type,payload,signature_valid,status,received_at) VALUES($1,$2,$1,'qualification',$3,true,'pending',$4)", params![webhook, run.actor, payload, now])?;
        owned.push(("webhook_events", webhook.clone()));
        let audit = Uuid::new_v4().to_string();
        db.execute("INSERT INTO audit_events(id,aggregate_type,aggregate_id,action,actor,details,created_at) VALUES($1,'creator',$2,'qualification',$3,$4,$5)", params![audit, creator_id, run.actor, payload, now])?;
        owned.push(("audit_events", audit.clone()));
        let export = run.root.join("records.json");
        let export_path = export.to_str().context("report path is not UTF-8")?;
        let actor = run.actor.clone();
        for (table, column, id, args) in [
            (
                "records",
                "data",
                creator_id.as_str(),
                vec!["standalone", "export", export_path],
            ),
            ("outbox", "payload", outbox.as_str(), vec!["sync", "outbox"]),
            (
                "webhook_events",
                "payload",
                webhook.as_str(),
                vec!["webhook", "log", "--connection", actor.as_str()],
            ),
            (
                "audit_events",
                "details",
                audit.as_str(),
                vec!["audit", "--kind", "creator", "--id", creator_id.as_str()],
            ),
        ] {
            let select = format!("SELECT {column} AS retained FROM {table} WHERE id=$1");
            let update = format!("UPDATE {table} SET {column}=$2 WHERE id=$1");
            let before: String = db.query_row(&select, params![id], |row| row.get("retained"))?;
            run.cli(&args, true)?;
            let damaged = "{";
            db.execute(&update, params![id, damaged])?;
            let refusal = run.cli(&args, false);
            let retained: Result<String, _> =
                db.query_row(&select, params![id], |row| row.get("retained"));
            // Restore our exact bytes even when the candidate wrongly accepted the damaged row.
            db.execute(&update, params![id, before])?;
            let message = refusal?;
            ensure!(
                message
                    .as_str()
                    .is_some_and(|message| message.contains(&format!("{table}.{column}"))),
                "refusal omitted its stored column: {message}"
            );
            ensure!(
                retained? == damaged,
                "reading damaged JSON rewrote the persisted row"
            );
            run.cli(&args, true)?;
            let restored: String = db.query_row(&select, params![id], |row| row.get("retained"))?;
            ensure!(restored == before, "recovery changed restored bytes");
        }
        Ok(())
    })();
    let mut cleanup_errors = Vec::new();
    for (table, id) in owned.into_iter().rev() {
        let cleaned: Result<()> = (|| {
            db.execute(&format!("DELETE FROM {table} WHERE id=$1"), params![id])?;
            let remaining: bool = db.query_row(
                &format!("SELECT EXISTS(SELECT id FROM {table} WHERE id=$1) AS remains"),
                params![id],
                |row| row.get("remains"),
            )?;
            ensure!(!remaining, "owned row {table}/{id} remains");
            Ok(())
        })();
        if let Err(error) = cleaned {
            cleanup_errors.push(format!("{error:#}"));
        }
    }
    if let Some(id) = creator {
        if let Err(error) = run.cli(&["creator", "remove", &id], true) {
            cleanup_errors.push(format!("{error:#}"));
        }
        if let Err(error) = run.cli(&["creator", "show", &id], false) {
            cleanup_errors.push(format!("{error:#}"));
        }
    }
    let cleanup_audit = db.execute(
        "DELETE FROM audit_events WHERE actor=$1",
        params![run.actor],
    );
    if let Err(error) = cleanup_audit {
        cleanup_errors.push(error.to_string());
    }
    fs::write(
        run.root.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "source_revision": revision, "binary_sha256": binary_sha256, "database": observed,
            "commands": run.commands, "error": result.as_ref().err().map(|error| format!("{error:#}")),
            "cleanup_errors": cleanup_errors,
            "scope": "Real persisted JSON reads through the CLI; no provider delivery or graphical qualification"
        }))?,
    )?;
    ensure!(
        cleanup_errors.is_empty(),
        "qualification cleanup failed: {cleanup_errors:?}"
    );
    result
}
