use super::*;

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
        if changed == 0 {
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
        let sql_limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map(params![Self::now(), sql_limit], Self::map_outbox)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }
}
