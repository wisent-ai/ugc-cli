//! First-use walkthrough. The journey Echo publishes for this product is the
//! one shipped in `onboarding_first_use.json` at the repository root, compiled
//! into the binary; this command walks that definition rather than a second
//! copy of the same words, so the screens an operator reads here are the
//! screens the control plane holds.
//!
//! Progress is recorded beside the ledger it describes, because the evidence
//! this journey waits for is the first campaign record in that database: a
//! scratch `--db` therefore rehearses the whole walk without touching the
//! progress of a working ledger. `--reset` discards the recorded attempt and
//! replays the journey from its entry screen in the same invocation.
//!
//! Nothing here contacts a creator and nothing here moves money. With
//! `--import`, the walk calls the same validated, transactional ledger import
//! as `standalone import`; otherwise it only reads local evidence.
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    db::{Record, Store},
    model::Campaign,
};

const PRODUCT_ID: &str = "ugc-cli";
const JOURNEY_ID: &str = "first-use";
const STATE_SCHEMA: &str = "ugc-cli.onboarding-state.v1";
const LEDGER_FACT: &str = "campaign_ledger_opened";
const FIRST_SUCCESS_FACT: &str = "campaign_record_created";

/// Where the ledger keeps its own first-success stamp. It is written by the
/// campaign write path, into the same database as the record that earned it,
/// so this walkthrough only ever reads it back.
pub(crate) const FIRST_SUCCESS_KEY: &str = "onboarding.first_use.campaign_record_created";

/// The published definition, embedded at build time from the file Echo's
/// publisher discovers at `origin/main`.
const DEFINITION: &str = include_str!("../onboarding_first_use.json");

/// The ledger accepted a campaign record: the first real result this product
/// produces. Recorded here, where the effect happens, rather than where a
/// walkthrough asks whether it happened. Only the first record is stamped; a
/// later campaign leaves the original observation alone.
pub(crate) fn record_first_success(store: &Store, campaign: &Campaign) -> Result<()> {
    if store.get_setting(FIRST_SUCCESS_KEY)?.is_some() {
        return Ok(());
    }
    store.set_setting(
        FIRST_SUCCESS_KEY,
        &json!({
            "product_id": PRODUCT_ID,
            "journey_id": JOURNEY_ID,
            "fact": FIRST_SUCCESS_FACT,
            "campaign_id": campaign.id,
            "observed_at": campaign.created_at,
        }),
    )
}

pub(crate) fn run(
    db_path: &Path,
    actor: &str,
    store: &Store,
    reset: bool,
    import_file: Option<&Path>,
    json_output: bool,
    yes: bool,
) -> Result<()> {
    let definition = canonical_definition()?;
    let revision = format!("{PRODUCT_ID}-{}", env!("CARGO_PKG_VERSION"));
    let progress = state_path(db_path);
    let mut state = load_or_start_state(&progress, db_path, actor, &definition, &revision, reset)?;
    let mut report = Report::new(&definition, db_path, reset, json_output);
    // Machine output and a piped stdin have no reader to press Enter.
    let unattended = yes || json_output || !io::stdin().is_terminal();

    if state.get("status").and_then(Value::as_str) == Some("completed") {
        report.finish(
            "completed",
            &state,
            "This ledger already accepted its first campaign record. Replay the journey with: ugc-cli onboarding --reset",
        );
        return report.emit();
    }

    loop {
        let screen_id = string_field(&state, "current_screen_id")
            .context("onboarding state has no current screen")?;
        let screen = screen_by_id(&definition, &screen_id)?.clone();
        report.render(&screen);

        match screen.get("screen_kind").and_then(Value::as_str) {
            Some("first_action") => {
                let counts = store.counts()?;
                report.note(&format!(
                    "Ledger {} is open and holds: {counts}",
                    db_path.display()
                ));
                let evidence = fact(LEDGER_FACT);
                advance(&definition, &screen, &mut state, &evidence, &revision, &progress)?
                    .context("an open ledger does not satisfy the published journey")?;
            }
            Some("first_success") => {
                if let Some(file) = import_file {
                    let records: Vec<Record> = serde_json::from_slice(
                        &fs::read(file)
                            .with_context(|| format!("cannot read {}", file.display()))?,
                    )
                    .context("onboarding import must be a canonical record export JSON array")?;
                    let result = store.import_records(&records, actor)?;
                    report.note(&format!("Import result: {result}"));
                    if result.get("applied").and_then(Value::as_bool) != Some(true) {
                        report.finish(
                            "import_refused",
                            &state,
                            "Resolve every reported conflict or rejection; the ledger was not changed.",
                        );
                        return report.emit();
                    }
                }
                let campaign = match first_campaign(store)? {
                    FirstCampaign::Accepted(campaign) => campaign,
                    FirstCampaign::None => {
                        report.note(
                            "This ledger holds no campaign record yet, so there is nothing to read back.",
                        );
                        report.finish(
                            "awaiting_campaign",
                            &state,
                            "Record the first campaign, then run: ugc-cli onboarding",
                        );
                        return report.emit();
                    }
                    FirstCampaign::Unreadable(id) => {
                        report.note(&format!(
                            "The recorded campaign {id} can no longer be read out of this ledger."
                        ));
                        report.finish(
                            "awaiting_campaign",
                            &state,
                            "Record a campaign this ledger can read back, then run: ugc-cli onboarding",
                        );
                        return report.emit();
                    }
                };
                report.campaign(&campaign);
                wait_for_enter(unattended, "Press Enter to finish onboarding. ")?;
                let evidence = fact(FIRST_SUCCESS_FACT);
                if !complete(&screen, &mut state, &evidence, &revision, &progress)? {
                    bail!("published first-success evidence was not satisfied");
                }
                report.finish(
                    "completed",
                    &state,
                    &format!(
                        "Give the campaign its first brief with: ugc-cli brief add --campaign {} --creative-angle <angle>",
                        campaign.id
                    ),
                );
                return report.emit();
            }
            Some(_) => {
                wait_for_enter(unattended, "Press Enter to continue. ")?;
                advance(&definition, &screen, &mut state, &Map::new(), &revision, &progress)?
                    .context("published journey has no eligible next screen")?;
            }
            None => bail!("published onboarding screen has no kind"),
        }
    }
}

/// One fact, asserted true: the only evidence shape the shipped journey uses.
fn fact(name: &str) -> Map<String, Value> {
    Map::from_iter([(name.to_string(), Value::Bool(true))])
}

/// What the ledger's first-success stamp resolves to. A stamp whose campaign
/// can no longer be read back is not evidence that the ledger holds it.
enum FirstCampaign {
    Accepted(Campaign),
    Unreadable(String),
    None,
}

fn first_campaign(store: &Store) -> Result<FirstCampaign> {
    let Some(record) = store.get_setting(FIRST_SUCCESS_KEY)? else {
        return Ok(FirstCampaign::None);
    };
    let id = record
        .get("campaign_id")
        .and_then(Value::as_str)
        .context("recorded first-success fact names no campaign")?;
    Ok(match store.get::<Campaign>("campaign", id) {
        Ok(campaign) => FirstCampaign::Accepted(campaign),
        Err(_) => FirstCampaign::Unreadable(id.to_string()),
    })
}

/// Screens as they are shown, plus the terminal verdict. Human output is
/// printed as the walk happens; `--json` collects the same walk into one
/// object, because a machine reader wants one document, not a transcript.
struct Report {
    journey_version: String,
    state_path: PathBuf,
    reset: bool,
    json: bool,
    steps: Vec<Value>,
    status: &'static str,
    current_screen_id: String,
    next: String,
}

impl Report {
    fn new(definition: &Value, db_path: &Path, reset: bool, json: bool) -> Self {
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

    fn render(&mut self, screen: &Value) {
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
    fn note(&mut self, text: &str) {
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
    fn campaign(&mut self, campaign: &Campaign) {
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

    fn finish(&mut self, status: &'static str, state: &Value, next: &str) {
        self.status = status;
        self.current_screen_id = string_field(state, "current_screen_id").unwrap_or_default();
        self.next = next.to_string();
    }

    fn emit(self) -> Result<()> {
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
fn canonical_definition() -> Result<Value> {
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

fn transitions(screen: &Value) -> &[Value] {
    screen
        .get("transitions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn screen_by_id<'a>(definition: &'a Value, screen_id: &str) -> Result<&'a Value> {
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
fn next_screen_id(screen: &Value) -> Option<&str> {
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
fn evidence_satisfied(screen: &Value, evidence: &Map<String, Value>) -> Result<bool> {
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

fn advance(
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

fn complete(
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

fn set(state: &mut Value, key: &str, value: Value) -> Result<()> {
    state
        .as_object_mut()
        .context("onboarding state is not an object")?
        .insert(key.to_string(), value);
    Ok(())
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Recorded progress for this ledger, or a fresh attempt. `reset` discards
/// what was recorded rather than resuming it, which is what replay means
/// here: the walk starts again at the journey's entry screen.
fn load_or_start_state(
    path: &Path,
    db_path: &Path,
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
        "subject_hash": subject_hash(db_path, actor)?,
        "attempt_id": Uuid::new_v4().to_string(),
        "current_screen_id": definition.get("entry_screen_id"),
        "status": "in_progress",
        "revision": revision,
    });
    save_state(path, &state)?;
    Ok(state)
}

fn save_state(path: &Path, state: &Value) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temporary = PathBuf::from(format!("{}.tmp-{}", path.display(), std::process::id()));
    fs::write(&temporary, format!("{}\n", serde_json::to_string(state)?))?;
    fs::rename(&temporary, &path)?;
    Ok(())
}

/// Progress belongs to the operator keeping this ledger, so it is stamped
/// with the ledger's resolved path and the acting operator.
fn subject_hash(db_path: &Path, actor: &str) -> Result<String> {
    let resolved = fs::canonicalize(db_path).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|cwd| cwd.join(db_path))
            .unwrap_or_else(|_| db_path.to_path_buf())
    });
    let identity = format!("{PRODUCT_ID}-onboarding\0{}\0{actor}", resolved.display());
    Ok(hex::encode(Sha256::digest(identity.as_bytes())))
}

/// The walk is progress against one ledger, not against this machine: a
/// scratch `--db` rehearses the journey without disturbing a working one.
fn state_path(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("onboarding-first-use.json")
}

fn wait_for_enter(unattended: bool, prompt: &str) -> Result<()> {
    if unattended {
        return Ok(());
    }
    print!("{prompt}");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(())
}
