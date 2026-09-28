use super::*;

/// Recorded progress for this ledger, or a fresh attempt. `reset` discards
/// what was recorded rather than resuming it, which is what replay means
/// here: the walk starts again at the journey's entry screen.
pub(crate) fn load_or_start_state(
    path: &Path,
    actor: &str,
    definition: &Value,
    revision: &str,
    reset: bool,
) -> Result<Value> {
    if !reset && path.exists() {
        let existing: Value = serde_json::from_str(&fs::read_to_string(&path)?)
            .with_context(|| format!("unreadable onboarding state: {}", path.display()))?;
        if existing.get("schema").and_then(Value::as_str) != Some(STATE_SCHEMA)
            || existing.get("product_id").and_then(Value::as_str) != Some(PRODUCT_ID)
            || existing.get("journey_id").and_then(Value::as_str) != Some(JOURNEY_ID)
        {
            bail!("stored onboarding state identity mismatch; use --reset to replace it");
        }
        // A journey republished with new screens invalidates a screen id that
        // no longer exists; resuming into it would show nothing at all.
        let current = string_field(&existing, "current_screen_id")
            .context("stored onboarding state has no current screen")?;
        if string_field(&existing, "journey_version") == string_field(definition, "journey_version")
            && screen_by_id(definition, &current).is_ok()
        {
            return Ok(existing);
        }
    }
    let state = json!({
        "schema": STATE_SCHEMA,
        "product_id": PRODUCT_ID,
        "journey_id": JOURNEY_ID,
        "journey_version": definition.get("journey_version"),
        "source_revision": definition.get("source_revision"),
        "subject_hash": subject_hash(actor),
        "attempt_id": Uuid::new_v4().to_string(),
        "current_screen_id": definition.get("entry_screen_id"),
        "status": "in_progress",
        "revision": revision,
    });
    save_state(path, &state)?;
    Ok(state)
}

pub(crate) fn save_state(path: &Path, state: &Value) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temporary = PathBuf::from(format!("{}.tmp-{}", path.display(), std::process::id()));
    fs::write(&temporary, format!("{}\n", serde_json::to_string(state)?))?;
    fs::rename(&temporary, &path)?;
    Ok(())
}

/// Progress belongs to the operator working the fleet ledger, so it is
/// stamped with that ledger's name and the acting operator.
pub(crate) fn subject_hash(actor: &str) -> String {
    let identity = format!(
        "{PRODUCT_ID}-onboarding\0fleet database {}\0{actor}",
        crate::db::Store::DATABASE
    );
    hex::encode(Sha256::digest(identity.as_bytes()))
}

/// Where this operator's walk is recorded: in the local state directory
/// beside the asset directory, since the ledger itself is the fleet's.
pub(crate) fn state_path(state_dir: &Path) -> PathBuf {
    state_dir.join("onboarding-first-use.json")
}

pub(crate) fn wait_for_enter(unattended: bool, prompt: &str) -> Result<()> {
    if unattended {
        return Ok(());
    }
    print!("{prompt}");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(())
}
