use super::*;

impl Report {
    pub(crate) fn new(definition: &Value, db_path: &Path, reset: bool, json: bool) -> Self {
        Self {
            journey_version: string_field(definition, "journey_version").unwrap_or_default(),
            state_path: state_path(db_path),
            reset,
            json,
            steps: Vec::new(),
            status: "in_progress",
            current_screen_id: String::new(),
            next: String::new(),
        }
    }

    pub(crate) fn render(&mut self, screen: &Value) {
        let presentation = screen.get("presentation");
        let title = presentation
            .and_then(|value| value.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("UGC CLI onboarding");
        let body = presentation
            .and_then(|value| value.get("body"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let command = presentation
            .and_then(|value| value.get("command"))
            .and_then(Value::as_str);
        if self.json {
            self.steps.push(json!({
                "screen_id": screen.get("screen_id"),
                "screen_kind": screen.get("screen_kind"),
                "title": title,
                "body": body,
                "command": command,
                "actions": screen.get("actions"),
            }));
            return;
        }
        println!("\n== {title} ==\n{body}");
        if let Some(command) = command {
            println!("command: {command}");
        }
    }

    /// A line about what this ledger actually holds, printed under the screen
    /// it qualifies. In `--json` it belongs to the step it followed.
    pub(crate) fn note(&mut self, text: &str) {
        if !self.json {
            println!("{text}");
            return;
        }
        if let Some(step) = self.steps.last_mut() {
            let notes = step
                .get_mut("notes")
                .and_then(Value::as_array_mut)
                .map(std::mem::take);
            let mut notes = notes.unwrap_or_default();
            notes.push(Value::String(text.to_string()));
            step["notes"] = Value::Array(notes);
        }
    }

    /// The campaign record read back out of the ledger: the first result of
    /// the product.
    pub(crate) fn campaign(&mut self, campaign: &Campaign) {
        if self.json {
            if let Some(step) = self.steps.last_mut() {
                step["campaign"] = json!({
                    "id": campaign.id,
                    "name": campaign.name,
                    "brand": campaign.brand,
                    "product": campaign.product,
                    "objective": campaign.objective,
                    "status": campaign.status,
                    "created_at": campaign.created_at,
                });
            }
            return;
        }
        println!(
            "read back campaign {}: {} for {} ({}), status {}, recorded {}",
            campaign.id,
            campaign.name,
            campaign.brand,
            campaign.product,
            campaign.status,
            campaign.created_at
        );
    }

    pub(crate) fn finish(&mut self, status: &'static str, state: &Value, next: &str) {
        self.status = status;
        self.current_screen_id = string_field(state, "current_screen_id").unwrap_or_default();
        self.next = next.to_string();
    }

    pub(crate) fn emit(self) -> Result<()> {
        if !self.json {
            println!("\nstatus: {}", self.status);
            println!("state: {}", self.state_path.display());
            println!("next: {}", self.next);
            return Ok(());
        }
        crate::output(&json!({
            "product_id": PRODUCT_ID,
            "journey_id": JOURNEY_ID,
            "journey_version": self.journey_version,
            "status": self.status,
            "current_screen_id": self.current_screen_id,
            "first_success_fact": FIRST_SUCCESS_FACT,
            "state_path": self.state_path,
            "reset": self.reset,
            "steps": self.steps,
            "next": self.next,
        }))
    }
}

/// The embedded definition, checked for the identity and the graph this
/// command relies on before a single screen is shown.
pub(crate) fn canonical_definition() -> Result<Value> {
    let definition: Value = serde_json::from_str(DEFINITION)
        .context("canonical onboarding journey is not valid JSON")?;
    if definition.get("schema_version").and_then(Value::as_u64) != Some(1)
        || definition.get("product_id").and_then(Value::as_str) != Some(PRODUCT_ID)
        || definition.get("journey_id").and_then(Value::as_str) != Some(JOURNEY_ID)
        || definition.get("first_success_fact").and_then(Value::as_str) != Some(FIRST_SUCCESS_FACT)
    {
        bail!("canonical onboarding journey identity mismatch");
    }
    let entry = string_field(&definition, "entry_screen_id")
        .context("canonical onboarding journey has no entry screen")?;
    let screens = definition
        .get("screens")
        .and_then(Value::as_array)
        .context("canonical onboarding journey has no screens")?;
    let mut ids: Vec<&str> = Vec::with_capacity(screens.len());
    for screen in screens {
        let id = screen
            .get("screen_id")
            .and_then(Value::as_str)
            .context("canonical onboarding screen has no id")?;
        if ids.contains(&id) {
            bail!("duplicate canonical onboarding screen id: {id}");
        }
        if screen.get("screen_kind").and_then(Value::as_str).is_none()
            || screen
                .get("presentation")
                .and_then(Value::as_object)
                .is_none()
        {
            bail!("canonical onboarding screen is incomplete: {id}");
        }
        ids.push(id);
    }
    if !ids.contains(&entry.as_str()) {
        bail!("canonical onboarding entry screen does not exist");
    }
    for screen in screens {
        for transition in transitions(screen) {
            let next = transition
                .get("next_screen_id")
                .and_then(Value::as_str)
                .context("canonical onboarding transition has no target")?;
            if !ids.contains(&next) {
                bail!("canonical onboarding transition target does not exist: {next}");
            }
        }
    }
    Ok(definition)
}

pub(crate) fn transitions(screen: &Value) -> &[Value] {
    screen
        .get("transitions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

pub(crate) fn screen_by_id<'a>(definition: &'a Value, screen_id: &str) -> Result<&'a Value> {
    definition
        .get("screens")
        .and_then(Value::as_array)
        .and_then(|screens| {
            screens
                .iter()
                .find(|screen| screen.get("screen_id").and_then(Value::as_str) == Some(screen_id))
        })
        .with_context(|| format!("published onboarding screen is unavailable: {screen_id}"))
}

/// The published edge out of this screen: highest priority wins, exactly as
/// the control plane's own selection does.
pub(crate) fn next_screen_id(screen: &Value) -> Option<&str> {
    transitions(screen)
        .iter()
        .max_by_key(|transition| {
            transition
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or_default()
        })
        .and_then(|transition| transition.get("next_screen_id").and_then(Value::as_str))
}

/// Whether the evidence this command gathered satisfies what the definition
/// requires of the screen. A screen with no rule is satisfied by arriving.
pub(crate) fn evidence_satisfied(screen: &Value, evidence: &Map<String, Value>) -> Result<bool> {
    let Some(rule) = screen
        .get("completion_evidence")
        .filter(|value| !value.is_null())
    else {
        return Ok(true);
    };
    if rule.get("kind").and_then(Value::as_str) != Some("fact")
        || rule.get("operator").and_then(Value::as_str) != Some("eq")
    {
        bail!("unsupported canonical onboarding evidence rule");
    }
    let name = rule
        .get("fact")
        .and_then(Value::as_str)
        .context("canonical onboarding evidence rule has no fact")?;
    let expected = rule
        .get("value")
        .context("canonical onboarding evidence rule has no expected value")?;
    Ok(evidence.get(name) == Some(expected))
}

pub(crate) fn advance(
    definition: &Value,
    screen: &Value,
    state: &mut Value,
    evidence: &Map<String, Value>,
    revision: &str,
    path: &Path,
) -> Result<Option<String>> {
    if !evidence_satisfied(screen, evidence)? {
        return Ok(None);
    }
    let Some(next) = next_screen_id(screen).map(str::to_string) else {
        return Ok(None);
    };
    screen_by_id(definition, &next)?;
    set(state, "current_screen_id", Value::String(next.clone()))?;
    set(state, "revision", Value::String(revision.to_string()))?;
    save_state(path, state)?;
    Ok(Some(next))
}

pub(crate) fn complete(
    screen: &Value,
    state: &mut Value,
    evidence: &Map<String, Value>,
    revision: &str,
    path: &Path,
) -> Result<bool> {
    if !evidence_satisfied(screen, evidence)? {
        return Ok(false);
    }
    set(state, "status", Value::String("completed".into()))?;
    set(state, "revision", Value::String(revision.to_string()))?;
    save_state(path, state)?;
    Ok(true)
}

pub(crate) fn set(state: &mut Value, key: &str, value: Value) -> Result<()> {
    state
        .as_object_mut()
        .context("onboarding state is not an object")?
        .insert(key.to_string(), value);
    Ok(())
}

pub(crate) fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}
