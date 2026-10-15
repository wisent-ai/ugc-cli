use super::*;

impl Report {
    pub(crate) fn new(definition: &Value, state_dir: &Path, reset: bool, json: bool) -> Self {
        Self {
            journey_version: string_field(definition, "journey_version"),
            state_path: state_path(state_dir),
            reset,
            json,
            steps: Vec::new(),
            status: "in_progress",
            current_screen_id: None,
            next: String::new(),
        }
    }

    /// Show one screen. The definition check already required its id, title
    /// and body, so a screen without them is refused here by the same reader.
    pub(crate) fn render(&mut self, screen: &Value) -> Result<()> {
        let id = screen
            .get("screen_id")
            .and_then(Value::as_str)
            .context("canonical onboarding screen has no id")?;
        let title = presentation_text(screen, id, "title")?;
        let body = presentation_text(screen, id, "body")?;
        let command = screen
            .get("presentation")
            .and_then(|value| value.get("command"))
            .and_then(Value::as_str);
        if self.json {
            self.steps.push(json!({
                "screen_id": id,
                "screen_kind": screen.get("screen_kind"),
                "title": title,
                "body": body,
                "command": command,
                "actions": screen.get("actions"),
            }));
            return Ok(());
        }
        println!("\n== {title} ==\n{body}");
        if let Some(command) = command {
            println!("command: {command}");
        }
        Ok(())
    }

    /// A line about what this ledger actually holds, printed under the screen
    /// it qualifies. In `--json` it belongs to the step it followed.
    pub(crate) fn note(&mut self, text: &str) {
        if !self.json {
            println!("{text}");
            return;
        }
        if let Some(step) = self.steps.last_mut() {
            let text = Value::String(text.to_string());
            match step.get_mut("notes").and_then(Value::as_array_mut) {
                Some(notes) => notes.push(text),
                None => step["notes"] = Value::Array(vec![text]),
            }
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
        self.current_screen_id = string_field(state, "current_screen_id");
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
    let Some(next) = next_screen_id(screen)?.map(str::to_string) else {
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
