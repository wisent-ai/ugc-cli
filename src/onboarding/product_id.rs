use super::*;

pub(crate) const PRODUCT_ID: &str = "ugc-cli";

pub(crate) const JOURNEY_ID: &str = "first-use";

pub(crate) const STATE_SCHEMA: &str = "ugc-cli.onboarding-state.v1";

pub(crate) const LEDGER_FACT: &str = "campaign_ledger_opened";

pub(crate) const FIRST_SUCCESS_FACT: &str = "campaign_record_created";

/// Where the ledger keeps its own first-success stamp. It is written by the
/// campaign write path, into the same database as the record that earned it,
/// so this walkthrough only ever reads it back.
pub(crate) const FIRST_SUCCESS_KEY: &str = "onboarding.first_use.campaign_record_created";

/// The published definition, embedded at build time from the file Echo's
/// publisher discovers at `origin/main`.
pub(crate) const DEFINITION: &str = include_str!("../../onboarding_first_use.json");

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
pub(crate) fn fact(name: &str) -> Map<String, Value> {
    Map::from_iter([(name.to_string(), Value::Bool(true))])
}

/// What the ledger's first-success stamp resolves to. A stamp whose campaign
/// can no longer be read back is not evidence that the ledger holds it.
pub(crate) enum FirstCampaign {
    Accepted(Campaign),
    Unreadable(String),
    None,
}

pub(crate) fn first_campaign(store: &Store) -> Result<FirstCampaign> {
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
pub(crate) struct Report {
    pub(crate) journey_version: String,
    pub(crate) state_path: PathBuf,
    pub(crate) reset: bool,
    pub(crate) json: bool,
    pub(crate) steps: Vec<Value>,
    pub(crate) status: &'static str,
    pub(crate) current_screen_id: String,
    pub(crate) next: String,
}
