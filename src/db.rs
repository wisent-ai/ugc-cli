use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use rusqlite::{Connection as Sqlite, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub kind: String,
    pub id: String,
    pub parent_id: Option<String>,
    pub secondary_id: Option<String>,
    pub status: String,
    pub external_id: Option<String>,
    pub data: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    pub id: String,
    pub kind: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub connection_id: Option<String>,
    pub payload: Value,
    pub status: String,
    pub attempts: i64,
    pub available_at: String,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct Store {
    db: Sqlite,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            bail!("database path must not be a symbolic link");
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let db = Sqlite::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        db.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS records (
                kind TEXT NOT NULL,
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                secondary_id TEXT,
                status TEXT NOT NULL,
                external_id TEXT,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS records_kind_parent ON records(kind, parent_id);
            CREATE INDEX IF NOT EXISTS records_kind_secondary ON records(kind, secondary_id);
            CREATE INDEX IF NOT EXISTS records_kind_status ON records(kind, status);
            CREATE UNIQUE INDEX IF NOT EXISTS records_external_unique
                ON records(kind, external_id) WHERE external_id IS NOT NULL;

            CREATE TABLE IF NOT EXISTS outbox (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                aggregate_type TEXT NOT NULL,
                aggregate_id TEXT NOT NULL,
                connection_id TEXT,
                payload TEXT NOT NULL,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                available_at TEXT NOT NULL,
                last_error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS outbox_due ON outbox(status, available_at);

            CREATE TABLE IF NOT EXISTS webhook_events (
                id TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                provider_event_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                signature_valid INTEGER NOT NULL,
                status TEXT NOT NULL,
                error TEXT,
                received_at TEXT NOT NULL,
                processed_at TEXT,
                UNIQUE(connection_id, provider_event_id)
            );

            CREATE TABLE IF NOT EXISTS audit_events (
                id TEXT PRIMARY KEY,
                aggregate_type TEXT NOT NULL,
                aggregate_id TEXT NOT NULL,
                action TEXT NOT NULL,
                actor TEXT NOT NULL,
                details TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS audit_aggregate ON audit_events(aggregate_type, aggregate_id, created_at);

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )?;
        protect_database_files(path)?;
        Ok(Self { db })
    }

    pub fn id() -> String {
        Uuid::new_v4().to_string()
    }

    pub fn now() -> String {
        Utc::now().to_rfc3339()
    }

    pub fn put<T: Serialize>(
        &self,
        kind: &str,
        id: &str,
        parent_id: Option<&str>,
        secondary_id: Option<&str>,
        status: &str,
        external_id: Option<&str>,
        value: &T,
        created_at: &str,
    ) -> Result<()> {
        let data = serde_json::to_string(value)?;
        let updated_at = Self::now();
        self.db.execute(
            r#"
            INSERT INTO records(kind,id,parent_id,secondary_id,status,external_id,data,created_at,updated_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
            ON CONFLICT(id) DO UPDATE SET
                parent_id=excluded.parent_id,
                secondary_id=excluded.secondary_id,
                status=excluded.status,
                external_id=excluded.external_id,
                data=excluded.data,
                updated_at=excluded.updated_at
            "#,
            params![kind, id, parent_id, secondary_id, status, external_id, data, created_at, updated_at],
        )?;
        Ok(())
    }

    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<T> {
        let raw: Option<String> = self
            .db
            .query_row(
                "SELECT data FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                |row| row.get("data"),
            )
            .optional()?;
        match raw {
            Some(data) => Ok(serde_json::from_str(&data)?),
            None => bail!("{kind} not found: {id}"),
        }
    }

    pub fn get_record(&self, kind: &str, id: &str) -> Result<Record> {
        self.db
            .query_row(
                "SELECT * FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                Self::map_record,
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("{kind} not found: {id}"))
    }

    pub fn find_external<T: DeserializeOwned>(
        &self,
        kind: &str,
        external_id: &str,
    ) -> Result<Option<T>> {
        let raw: Option<String> = self
            .db
            .query_row(
                "SELECT data FROM records WHERE kind=?1 AND external_id=?2",
                params![kind, external_id],
                |row| row.get("data"),
            )
            .optional()?;
        raw.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }

    pub fn list<T: DeserializeOwned>(
        &self,
        kind: &str,
        parent_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<T>> {
        let mut sql = String::from("SELECT data FROM records WHERE kind=?1");
        if parent_id.is_some() {
            sql.push_str(" AND parent_id=?2");
            if status.is_some() {
                sql.push_str(" AND status=?3");
            }
        } else if status.is_some() {
            sql.push_str(" AND status=?2");
        }
        sql.push_str(" ORDER BY created_at DESC");
        let mut stmt = self.db.prepare(&sql)?;
        let values: Vec<String> = match (parent_id, status) {
            (Some(parent), Some(state)) => vec![kind.into(), parent.into(), state.into()],
            (Some(parent), None) => vec![kind.into(), parent.into()],
            (None, Some(state)) => vec![kind.into(), state.into()],
            (None, None) => vec![kind.into()],
        };
        let rows = stmt.query_map(rusqlite::params_from_iter(values), |row| {
            row.get::<_, String>("data")
        })?;
        let mut output = Vec::new();
        for row in rows {
            output.push(serde_json::from_str(&row?)?);
        }
        Ok(output)
    }

    pub fn delete(&self, kind: &str, id: &str) -> Result<()> {
        let changed = self.db.execute(
            "DELETE FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
        )?;
        if changed == "".len() {
            bail!("{kind} not found: {id}");
        }
        Ok(())
    }

    pub fn audit(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        action: &str,
        actor: &str,
        details: &Value,
    ) -> Result<()> {
        self.db.execute(
            "INSERT INTO audit_events(id,aggregate_type,aggregate_id,action,actor,details,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![Self::id(), aggregate_type, aggregate_id, action, actor, details.to_string(), Self::now()],
        )?;
        Ok(())
    }

    pub fn audit_log(
        &self,
        aggregate_type: Option<&str>,
        aggregate_id: Option<&str>,
    ) -> Result<Vec<Value>> {
        let (sql, values): (&str, Vec<String>) = match (aggregate_type, aggregate_id) {
            (Some(kind), Some(id)) => (
                "SELECT * FROM audit_events WHERE aggregate_type=?1 AND aggregate_id=?2 ORDER BY created_at DESC",
                vec![kind.into(), id.into()],
            ),
            (Some(kind), None) => (
                "SELECT * FROM audit_events WHERE aggregate_type=?1 ORDER BY created_at DESC",
                vec![kind.into()],
            ),
            _ => (
                "SELECT * FROM audit_events ORDER BY created_at DESC",
                Vec::new(),
            ),
        };
        let mut stmt = self.db.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values), |row| {
            let details: String = row.get("details")?;
            Ok(serde_json::json!({
                "id": row.get::<_, String>("id")?,
                "aggregate_type": row.get::<_, String>("aggregate_type")?,
                "aggregate_id": row.get::<_, String>("aggregate_id")?,
                "action": row.get::<_, String>("action")?,
                "actor": row.get::<_, String>("actor")?,
                "details": serde_json::from_str::<Value>(&details).unwrap_or(Value::String(details)),
                "created_at": row.get::<_, String>("created_at")?,
            }))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn enqueue(
        &self,
        kind: &str,
        aggregate_type: &str,
        aggregate_id: &str,
        connection_id: Option<&str>,
        payload: &Value,
    ) -> Result<String> {
        let id = Self::id();
        let now = Self::now();
        self.db.execute(
            "INSERT INTO outbox(id,kind,aggregate_type,aggregate_id,connection_id,payload,status,attempts,available_at,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,'pending',0,?7,?7,?7)",
            params![id, kind, aggregate_type, aggregate_id, connection_id, payload.to_string(), now],
        )?;
        Ok(id)
    }

    pub fn due_outbox(&self, limit: usize) -> Result<Vec<OutboxItem>> {
        let mut stmt = self.db.prepare(
            "SELECT * FROM outbox WHERE status IN ('pending','retry') AND available_at<=?1 ORDER BY created_at LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![Self::now(), limit], Self::map_outbox)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn list_outbox(&self, status: Option<&str>) -> Result<Vec<OutboxItem>> {
        let (sql, values): (&str, Vec<String>) = match status {
            Some(state) => (
                "SELECT * FROM outbox WHERE status=?1 ORDER BY created_at DESC",
                vec![state.into()],
            ),
            None => ("SELECT * FROM outbox ORDER BY created_at DESC", Vec::new()),
        };
        let mut stmt = self.db.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values), Self::map_outbox)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn complete_outbox(&self, id: &str) -> Result<()> {
        self.db.execute(
            "UPDATE outbox SET status='completed',updated_at=?2,last_error=NULL WHERE id=?1",
            params![id, Self::now()],
        )?;
        Ok(())
    }

    pub fn fail_outbox(&self, id: &str, attempts: i64, error: &str, terminal: bool) -> Result<()> {
        let status = if terminal { "dead" } else { "retry" };
        self.db.execute(
            "UPDATE outbox SET status=?2,attempts=?3,last_error=?4,available_at=datetime('now','+1 minute'),updated_at=?5 WHERE id=?1",
            params![id, status, attempts, error, Self::now()],
        )?;
        Ok(())
    }

    pub fn replay_outbox(&self, id: &str) -> Result<()> {
        self.db.execute(
            "UPDATE outbox SET status='pending',attempts=0,last_error=NULL,available_at=?2,updated_at=?2 WHERE id=?1",
            params![id, Self::now()],
        )?;
        Ok(())
    }

    pub fn store_webhook(
        &self,
        connection_id: &str,
        provider_event_id: &str,
        event_type: &str,
        payload: &Value,
        signature_valid: bool,
    ) -> Result<bool> {
        let changed = self.db.execute(
            "INSERT OR IGNORE INTO webhook_events(id,connection_id,provider_event_id,event_type,payload,signature_valid,status,received_at) VALUES(?1,?2,?3,?4,?5,?6,'pending',?7)",
            params![Self::id(), connection_id, provider_event_id, event_type, payload.to_string(), signature_valid, Self::now()],
        )?;
        Ok(changed != "".len())
    }

    pub fn finish_webhook(
        &self,
        connection_id: &str,
        provider_event_id: &str,
        error: Option<&str>,
    ) -> Result<()> {
        let status = if error.is_some() {
            "failed"
        } else {
            "processed"
        };
        self.db.execute(
            "UPDATE webhook_events SET status=?3,error=?4,processed_at=?5 WHERE connection_id=?1 AND provider_event_id=?2",
            params![connection_id, provider_event_id, status, error, Self::now()],
        )?;
        Ok(())
    }

    pub fn webhook_log(&self, connection_id: Option<&str>) -> Result<Vec<Value>> {
        let (sql, values): (&str, Vec<String>) = match connection_id {
            Some(id) => (
                "SELECT * FROM webhook_events WHERE connection_id=?1 ORDER BY received_at DESC",
                vec![id.into()],
            ),
            None => (
                "SELECT * FROM webhook_events ORDER BY received_at DESC",
                Vec::new(),
            ),
        };
        let mut stmt = self.db.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values), |row| {
            let payload: String = row.get("payload")?;
            Ok(serde_json::json!({
                "id": row.get::<_, String>("id")?,
                "connection_id": row.get::<_, String>("connection_id")?,
                "provider_event_id": row.get::<_, String>("provider_event_id")?,
                "event_type": row.get::<_, String>("event_type")?,
                "payload": serde_json::from_str::<Value>(&payload).unwrap_or(Value::String(payload)),
                "signature_valid": row.get::<_, bool>("signature_valid")?,
                "status": row.get::<_, String>("status")?,
                "error": row.get::<_, Option<String>>("error")?,
                "received_at": row.get::<_, String>("received_at")?,
                "processed_at": row.get::<_, Option<String>>("processed_at")?,
            }))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn set_setting(&self, key: &str, value: &Value) -> Result<()> {
        self.db.execute(
            "INSERT INTO settings(key,value,updated_at) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
            params![key, value.to_string(), Self::now()],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<Value>> {
        let raw: Option<String> = self
            .db
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get("value")
            })
            .optional()?;
        raw.map(|value| serde_json::from_str(&value).map_err(Into::into))
            .transpose()
    }

    pub fn all_records(&self) -> Result<Vec<Record>> {
        let mut stmt = self
            .db
            .prepare("SELECT * FROM records ORDER BY kind,created_at,id")?;
        let rows = stmt.query_map([], Self::map_record)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn import_records(&self, records: &[Record]) -> Result<Value> {
        let mut imported = Vec::new();
        for record in records {
            if record.kind.trim().is_empty() || record.id.trim().is_empty() {
                bail!("import record kind and id are required");
            }
            self.put(
                &record.kind,
                &record.id,
                record.parent_id.as_deref(),
                record.secondary_id.as_deref(),
                &record.status,
                record.external_id.as_deref(),
                &record.data,
                &record.created_at,
            )?;
            imported.push(record.id.clone());
        }
        Ok(serde_json::json!({"imported": imported}))
    }

    pub fn counts(&self) -> Result<Value> {
        let mut stmt = self
            .db
            .prepare("SELECT kind,COUNT(*) AS count FROM records GROUP BY kind ORDER BY kind")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>("kind")?, row.get::<_, i64>("count")?))
        })?;
        let mut map = serde_json::Map::new();
        for row in rows {
            let (kind, count) = row?;
            map.insert(kind, Value::from(count));
        }
        Ok(Value::Object(map))
    }

    fn map_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
        let data: String = row.get("data")?;
        Ok(Record {
            kind: row.get("kind")?,
            id: row.get("id")?,
            parent_id: row.get("parent_id")?,
            secondary_id: row.get("secondary_id")?,
            status: row.get("status")?,
            external_id: row.get("external_id")?,
            data: serde_json::from_str(&data).unwrap_or(Value::String(data)),
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    fn map_outbox(row: &rusqlite::Row<'_>) -> rusqlite::Result<OutboxItem> {
        let payload: String = row.get("payload")?;
        Ok(OutboxItem {
            id: row.get("id")?,
            kind: row.get("kind")?,
            aggregate_type: row.get("aggregate_type")?,
            aggregate_id: row.get("aggregate_id")?,
            connection_id: row.get("connection_id")?,
            payload: serde_json::from_str(&payload).unwrap_or(Value::String(payload)),
            status: row.get("status")?,
            attempts: row.get("attempts")?,
            available_at: row.get("available_at")?,
            last_error: row.get("last_error")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

#[cfg(unix)]
fn protect_database_files(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = u32::from_str_radix("600", "security".len())?;
    for target in [
        path.to_path_buf(),
        database_sidecar(path, "-wal"),
        database_sidecar(path, "-shm"),
    ] {
        if target.exists() {
            fs::set_permissions(&target, fs::Permissions::from_mode(mode))
                .with_context(|| format!("cannot protect {}", target.display()))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn database_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(not(unix))]
fn protect_database_files(_path: &Path) -> Result<()> {
    Ok(())
}
