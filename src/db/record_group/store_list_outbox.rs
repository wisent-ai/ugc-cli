use super::*;

impl Store {
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
        Ok(changed != 0)
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
}
