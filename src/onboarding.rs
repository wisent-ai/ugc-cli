use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use tokio::runtime::Builder;
use uuid::Uuid;
use wisent_onboarding_client::{
    FileStorage, IntegrationTransport, JourneyClient, JourneyError, OfflineTransport,
    ProgressStatus, ScopeKind, Transport, bundle_from_canonical,
};

use crate::{
    db::Store,
    model::Campaign,
};

const PRODUCT_ID: &str = "ugc-cli";
const JOURNEY_ID: &str = "first-use";
const JOURNEY_VERSION: &str = "2026-08-04.1";
const FIRST_SUCCESS_FACT: &str = "campaign_record_created";
const STADO_TOKEN_ENV: &str = "UGC_CLI_STADO_INTEGRATION_TOKEN";
const FALLBACK_VERSION_ID: &str = "6aab4816-5057-4ea1-9acf-da233ecea9d4";

const FALLBACK_DEFINITION: &str = r#"{"schema_version":1,"product_id":"ugc-cli","journey_id":"first-use","journey_version":"2026-08-04.1","entry_screen_id":"campaign-system","first_success_fact":"campaign_record_created","published_at":"2026-08-04T00:00:00Z","source_revision":"ugc-cli:first-use:2026-08-04.1","screens":[{"screen_id":"campaign-system","screen_kind":"education","title_key":"campaign_system.title","body_key":"campaign_system.body","required":true,"actions":["continue"],"transitions":[{"next_screen_id":"campaign-create","reason_code":"campaign_system_understood","priority":1}],"presentation":{"surface":"cli","ordinal":1}},{"screen_id":"campaign-create","screen_kind":"action","title_key":"campaign_create.title","body_key":"campaign_create.body","required":true,"completion_evidence":{"kind":"fact","fact":"campaign_record_created","operator":"eq","value":true},"actions":["create_campaign"],"transitions":[{"next_screen_id":"campaign-created","reason_code":"campaign_record_created","priority":1,"condition":{"kind":"fact","fact":"campaign_record_created","operator":"eq","value":true}}],"presentation":{"surface":"cli","ordinal":2}},{"screen_id":"campaign-created","screen_kind":"success","title_key":"campaign_created.title","body_key":"campaign_created.body","required":true,"completion_evidence":{"kind":"fact","fact":"campaign_record_created","operator":"eq","value":true},"actions":["inspect_campaign"],"transitions":[],"presentation":{"surface":"cli","ordinal":3}}],"analytics_contract":{"contract_version":"1","surface":"cli","exposure_event":"onboarding_step_viewed","primary_action_event":"onboarding_step_completed","completion_event":"onboarding_completed","first_success_event":"onboarding_first_success_observed"}}"#;

type Client = JourneyClient<Box<dyn Transport>, FileStorage>;

#[derive(Serialize)]
pub(crate) struct OnboardingView {
    product_id: &'static str,
    journey_id: &'static str,
    journey_version: String,
    status: &'static str,
    screen_id: String,
    title: &'static str,
    body: &'static str,
    commands: Vec<String>,
    campaign_id: Option<String>,
}

pub(crate) fn run(
    db_path: &Path,
    actor: &str,
    store: &Store,
    advance: bool,
) -> Result<OnboardingView> {
    let campaign = newest_campaign(store)?;
    let evidence = campaign_evidence(campaign.as_ref());
    let revision = evidence_revision(campaign.as_ref());
    let mut client = client(db_path, actor)?;
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the onboarding runtime")?;

    runtime.block_on(async {
        client.start(&revision).await?;
        if campaign.is_some() {
            drive_to_completion(&mut client, &evidence, &revision).await?;
        } else if advance && client.progress().is_some_and(|progress| progress.status != ProgressStatus::Completed) {
            client.advance(&evidence, &revision).await?;
        }
        client.expose(&revision).await?;
        client.flush().await?;
        Ok::<(), JourneyError>(())
    })?;

    render(&client, campaign.as_ref())
}

pub(crate) fn campaign_created(
    db_path: &Path,
    actor: &str,
    campaign: &Campaign,
) -> Result<()> {
    let evidence = campaign_evidence(Some(campaign));
    let revision = evidence_revision(Some(campaign));
    let mut client = client(db_path, actor)?;
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the onboarding runtime")?;

    runtime.block_on(async {
        client.start(&revision).await?;
        drive_to_completion(&mut client, &evidence, &revision).await?;
        client.flush().await
    })?;
    Ok(())
}

async fn drive_to_completion(
    client: &mut Client,
    evidence: &BTreeMap<String, Value>,
    revision: &str,
) -> Result<(), JourneyError> {
    if client.progress().is_some_and(|progress| progress.status == ProgressStatus::Completed) {
        return Ok(());
    }

    while client.advance(evidence, revision).await?.is_some() {}
    client.complete(evidence, revision).await?;
    Ok(())
}

fn client(db_path: &Path, actor: &str) -> Result<Client> {
    let fallback_id = Uuid::parse_str(FALLBACK_VERSION_ID).context("invalid bundled journey identity")?;
    let fallback = bundle_from_canonical(FALLBACK_DEFINITION, fallback_id)?;
    if fallback.definition.journey_version != JOURNEY_VERSION
        || fallback.definition.first_success_fact != FIRST_SUCCESS_FACT
    {
        bail!("invalid bundled onboarding contract");
    }

    let transport: Box<dyn Transport> = match (
        env::var("STADO_INTEGRATION_API_URL").ok(),
        env::var(STADO_TOKEN_ENV).ok(),
    ) {
        (Some(base_url), Some(token)) => Box::new(
            IntegrationTransport::new(&base_url, token)
                .context("invalid onboarding integration transport configuration")?,
        ),
        _ => Box::new(OfflineTransport),
    };

    Ok(JourneyClient::new(
        PRODUCT_ID,
        JOURNEY_ID,
        subject_hash(db_path, actor)?,
        ScopeKind::Workload,
        transport,
        FileStorage::new(storage_path(db_path)),
        fallback,
    )?)
}

fn subject_hash(db_path: &Path, actor: &str) -> Result<String> {
    let absolute = if db_path.is_absolute() {
        db_path.to_path_buf()
    } else {
        env::current_dir()?.join(db_path)
    };
    let stable_path = fs::canonicalize(db_path).unwrap_or(absolute);
    let identity = format!("{PRODUCT_ID}\0{}\0{actor}", stable_path.display());
    Ok(hex::encode(Sha256::digest(identity.as_bytes())))
}

fn storage_path(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("onboarding.json")
}

fn newest_campaign(store: &Store) -> Result<Option<Campaign>> {
    Ok(store
        .list::<Campaign>("campaign", None, None)?
        .into_iter()
        .next())
}

fn campaign_evidence(campaign: Option<&Campaign>) -> BTreeMap<String, Value> {
    let mut evidence = BTreeMap::new();
    if let Some(campaign) = campaign {
        evidence.insert(FIRST_SUCCESS_FACT.into(), Value::Bool(true));
        evidence.insert("campaign_id".into(), Value::String(campaign.id.clone()));
    }
    evidence
}

fn evidence_revision(campaign: Option<&Campaign>) -> String {
    campaign
        .map(|campaign| format!("campaign:{}:{}", campaign.id, campaign.updated_at))
        .unwrap_or_else(|| "campaign:none".into())
}

fn render(client: &Client, campaign: Option<&Campaign>) -> Result<OnboardingView> {
    let bundle = client.bundle().context("onboarding bundle was not loaded")?;
    let progress = client.progress().context("onboarding progress was not loaded")?;
    let screen = bundle
        .definition
        .screens
        .iter()
        .find(|screen| screen.screen_id == progress.current_screen_id)
        .context("onboarding screen was not found")?;
    let (title, body) = content(&screen.title_key, &screen.body_key);
    let commands = screen
        .actions
        .iter()
        .filter_map(|action| command(action, campaign))
        .collect();

    Ok(OnboardingView {
        product_id: PRODUCT_ID,
        journey_id: JOURNEY_ID,
        journey_version: bundle.definition.journey_version.clone(),
        status: status(progress.status),
        screen_id: screen.screen_id.clone(),
        title,
        body,
        commands,
        campaign_id: campaign.map(|campaign| campaign.id.clone()),
    })
}

fn content(title_key: &str, body_key: &str) -> (&'static str, &'static str) {
    match (title_key, body_key) {
        ("campaign_system.title", "campaign_system.body") => (
            "Keep the campaign as the system of record",
            "A campaign record anchors its brief, creators, assignments, submissions, rights, payments, and provider publications. Create it first so every later operation has one durable campaign ID.",
        ),
        ("campaign_create.title", "campaign_create.body") => (
            "Create the first real campaign record",
            "Run the campaign create command with real working details. The saved record—not advancing this guide—is the evidence that completes first use.",
        ),
        ("campaign_created.title", "campaign_created.body") => (
            "Your campaign record is ready",
            "Inspect the persisted record by ID, then use that same ID when adding its brief and assigning creators.",
        ),
        _ => (
            "Campaign first use",
            "Follow the product commands for this campaign journey step.",
        ),
    }
}

fn command(action: &str, campaign: Option<&Campaign>) -> Option<String> {
    match action {
        "continue" => Some("ugc onboarding next".into()),
        "create_campaign" => Some(
            "ugc campaign create --name <name> --brand <brand> --product <product> --objective <objective>"
                .into(),
        ),
        "inspect_campaign" => campaign
            .map(|campaign| format!("ugc campaign show {}", campaign.id))
            .or_else(|| Some("ugc campaign list".into())),
        _ => None,
    }
}

fn status(status: ProgressStatus) -> &'static str {
    match status {
        ProgressStatus::InProgress => "in_progress",
        ProgressStatus::Skipped => "skipped",
        ProgressStatus::Completed => "completed",
        ProgressStatus::Abandoned => "abandoned",
        ProgressStatus::Reset => "reset",
    }
}
