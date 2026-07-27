mod db;
mod ecosystem;
mod media;
mod model;
mod provider;
mod secret;
mod server;
mod service;
mod standalone;
mod standalone_server;
mod sync;

use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    db::{Record, Store},
    media::QcPolicy,
    model::{
        Assignment, Brief, Campaign, Connection, Creator, CreatorIdentity, DiscoveryQuery, Message,
        Payment, Publication, Shipment, ShippingAddress, Submission, UsageRights,
    },
    service::UgcService,
    standalone::{CreatorSeed, MetricInput, StandaloneService},
};

#[derive(Parser)]
#[command(
    name = "ugc",
    version,
    about = "Provider-agnostic UGC campaign operations"
)]
struct Cli {
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[arg(long, global = true)]
    asset_dir: Option<PathBuf>,
    #[arg(long, global = true, default_value = "cli")]
    actor: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Connection(ConnectionArgs),
    Campaign(CampaignArgs),
    Brief(BriefArgs),
    Creator(CreatorArgs),
    Assignment(AssignmentArgs),
    Shipment(ShipmentArgs),
    Submission(SubmissionArgs),
    Asset(AssetArgs),
    Rights(RightsArgs),
    Payment(PaymentArgs),
    Message(MessageArgs),
    Sync(SyncArgs),
    Webhook(WebhookArgs),
    Weles(WelesArgs),
    Skarbiec(SkarbiecArgs),
    Brama(BramaArgs),
    Standalone(StandaloneArgs),
    Audit(AuditArgs),
    Diagnostics,
}

#[derive(Args)]
struct ConnectionArgs {
    #[command(subcommand)]
    command: ConnectionCommand,
}

#[derive(Subcommand)]
enum ConnectionCommand {
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long = "token-source", visible_alias = "token-env")]
        token_source: Option<String>,
        #[arg(long = "webhook-secret-source", visible_alias = "webhook-secret-env")]
        webhook_secret_source: Option<String>,
        #[arg(long)]
        external_account_id: Option<String>,
    },
    List,
    Show {
        id: String,
    },
    Health {
        id: String,
    },
    Remove {
        id: String,
    },
}

#[derive(Args)]
struct CampaignArgs {
    #[command(subcommand)]
    command: CampaignCommand,
}

#[derive(Subcommand)]
enum CampaignCommand {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        brand: String,
        #[arg(long)]
        product: String,
        #[arg(long, default_value = "")]
        objective: String,
        #[arg(long, value_delimiter = ',')]
        markets: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        languages: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        channels: Vec<String>,
        #[arg(long)]
        budget_minor: Option<i64>,
        #[arg(long, default_value = "USD")]
        currency: String,
        #[arg(long)]
        deadline: Option<String>,
    },
    List {
        #[arg(long)]
        status: Option<String>,
    },
    Show {
        id: String,
    },
    Status {
        id: String,
        status: String,
    },
    Publish {
        id: String,
        #[arg(long)]
        brief: String,
        #[arg(long)]
        connection: String,
    },
    Publications {
        id: String,
    },
}

#[derive(Args)]
struct BriefArgs {
    #[command(subcommand)]
    command: BriefCommand,
}

#[derive(Subcommand)]
enum BriefCommand {
    Add {
        #[arg(long)]
        campaign: String,
        #[arg(long, default_value = "ugc_content")]
        service_type: String,
        #[arg(long)]
        creative_angle: String,
        #[arg(long, value_delimiter = ',')]
        requirements: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        forbidden_claims: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        required_shots: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        talking_points: Vec<String>,
        #[arg(long)]
        cta: Option<String>,
        #[arg(long)]
        duration_min_ms: Option<i64>,
        #[arg(long)]
        duration_max_ms: Option<i64>,
        #[arg(long, value_delimiter = ',')]
        aspect_ratios: Vec<String>,
        #[arg(long)]
        raw_footage_required: bool,
        #[arg(long)]
        revision_limit: Option<i64>,
        #[arg(long, default_value = "{}")]
        rights_requirements: String,
    },
    List {
        #[arg(long)]
        campaign: String,
    },
    Show {
        id: String,
    },
    Approve {
        id: String,
    },
}

#[derive(Args)]
struct CreatorArgs {
    #[command(subcommand)]
    command: CreatorCommand,
}

#[derive(Subcommand)]
enum CreatorCommand {
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        email: Option<String>,
        #[arg(long, value_delimiter = ',')]
        languages: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        markets: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        niches: Vec<String>,
        #[arg(long, default_value = "{}")]
        metadata: String,
    },
    Verify {
        id: String,
        #[arg(long, default_value = "{}")]
        metadata: String,
    },
    List,
    Show {
        id: String,
    },
    Identity {
        #[arg(long)]
        creator: String,
        #[arg(long)]
        connection: Option<String>,
        #[arg(long)]
        platform: String,
        #[arg(long)]
        external_id: String,
        #[arg(long)]
        profile_url: Option<String>,
        #[arg(long, default_value = "{}")]
        metadata: String,
    },
    Identities {
        #[arg(long)]
        creator: String,
    },
}

#[derive(Args)]
struct AssignmentArgs {
    #[command(subcommand)]
    command: AssignmentCommand,
}

#[derive(Subcommand)]
enum AssignmentCommand {
    Create {
        #[arg(long)]
        campaign: String,
        #[arg(long)]
        brief: String,
        #[arg(long)]
        creator: String,
        #[arg(long)]
        connection: Option<String>,
        #[arg(long)]
        compensation_minor: Option<i64>,
        #[arg(long, default_value = "USD")]
        currency: String,
        #[arg(long, default_value = "none")]
        payment_owner: String,
        #[arg(long)]
        deadline: Option<String>,
        #[arg(long)]
        shipping_required: bool,
        #[arg(long)]
        revision_limit: Option<i64>,
        #[arg(long)]
        external_id: Option<String>,
    },
    List {
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    Show {
        id: String,
    },
    Status {
        id: String,
        status: String,
    },
}

#[derive(Args)]
struct ShipmentArgs {
    #[command(subcommand)]
    command: ShipmentCommand,
}

#[derive(Subcommand)]
enum ShipmentCommand {
    Update {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        carrier: Option<String>,
        #[arg(long)]
        tracking: Option<String>,
        #[arg(long)]
        product_variant: Option<String>,
        #[arg(long)]
        address_json: Option<String>,
    },
    List {
        #[arg(long)]
        assignment: String,
    },
}

#[derive(Args)]
struct SubmissionArgs {
    #[command(subcommand)]
    command: SubmissionCommand,
}

#[derive(Subcommand)]
enum SubmissionCommand {
    Add {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        external_id: Option<String>,
    },
    List {
        #[arg(long)]
        assignment: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    Show {
        id: String,
    },
    Review {
        id: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        feedback: Option<String>,
    },
}

#[derive(Args)]
struct AssetArgs {
    #[command(subcommand)]
    command: AssetCommand,
}

#[derive(Subcommand)]
enum AssetCommand {
    Import {
        path: PathBuf,
        #[arg(long)]
        submission: Option<String>,
        #[arg(long, default_value = "final")]
        role: String,
        #[arg(long)]
        source_url: Option<String>,
    },
    List {
        #[arg(long)]
        submission: Option<String>,
    },
    Show {
        id: String,
    },
    Qc {
        id: String,
        #[arg(long)]
        mime_prefix: Option<String>,
        #[arg(long)]
        min_duration_ms: Option<i64>,
        #[arg(long)]
        max_duration_ms: Option<i64>,
        #[arg(long, value_delimiter = ',')]
        aspect_ratios: Vec<String>,
        #[arg(long)]
        max_bytes: Option<i64>,
    },
}

#[derive(Args)]
struct RightsArgs {
    #[command(subcommand)]
    command: RightsCommand,
}

#[derive(Subcommand)]
enum RightsCommand {
    Grant {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        owner: String,
        #[arg(long)]
        license_type: String,
        #[arg(long)]
        organic: bool,
        #[arg(long)]
        paid_ads: bool,
        #[arg(long)]
        whitelisting: bool,
        #[arg(long)]
        editing: bool,
        #[arg(long)]
        ai_transform: bool,
        #[arg(long)]
        raw_footage: bool,
        #[arg(long, value_delimiter = ',')]
        territories: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        channels: Vec<String>,
        #[arg(long)]
        starts_at: Option<String>,
        #[arg(long)]
        expires_at: Option<String>,
        #[arg(long)]
        model_release: bool,
        #[arg(long)]
        music_cleared: bool,
        #[arg(long)]
        contract_url: Option<String>,
    },
    Check {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        channel: String,
        #[arg(long)]
        territory: Option<String>,
        #[arg(long)]
        paid: bool,
        #[arg(long)]
        at: Option<String>,
    },
    List {
        #[arg(long)]
        assignment: String,
    },
}

#[derive(Args)]
struct PaymentArgs {
    #[command(subcommand)]
    command: PaymentCommand,
}

#[derive(Subcommand)]
enum PaymentCommand {
    Create {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        submission: Option<String>,
        #[arg(long)]
        amount_minor: i64,
        #[arg(long, default_value = "USD")]
        currency: String,
        #[arg(long)]
        external_id: Option<String>,
    },
    List {
        #[arg(long)]
        assignment: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    Show {
        id: String,
    },
    Status {
        id: String,
        status: String,
        #[arg(long)]
        external_id: Option<String>,
        #[arg(long)]
        error: Option<String>,
    },
}

#[derive(Args)]
struct MessageArgs {
    #[command(subcommand)]
    command: MessageCommand,
}

#[derive(Subcommand)]
enum MessageCommand {
    Send {
        #[arg(long)]
        assignment: String,
        #[arg(long, default_value = "outbound")]
        direction: String,
        #[arg(long, default_value = "provider")]
        channel: String,
        #[arg(long)]
        body: String,
    },
    List {
        #[arg(long)]
        assignment: String,
    },
}

#[derive(Args)]
struct SyncArgs {
    #[command(subcommand)]
    command: SyncCommand,
}

#[derive(Subcommand)]
enum SyncCommand {
    Run {
        #[arg(long)]
        limit: Option<usize>,
    },
    Connection {
        id: String,
    },
    Outbox {
        #[arg(long)]
        status: Option<String>,
    },
    Replay {
        id: String,
    },
}

#[derive(Args)]
struct WebhookArgs {
    #[command(subcommand)]
    command: WebhookCommand,
}

#[derive(Subcommand)]
enum WebhookCommand {
    Ingest {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        signature: Option<String>,
    },
    Serve {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        bind: String,
        #[arg(long)]
        once: bool,
    },
    Log {
        #[arg(long)]
        connection: Option<String>,
    },
}

#[derive(Args)]
struct WelesArgs {
    #[command(subcommand)]
    command: WelesCommand,
}

#[derive(Subcommand)]
enum WelesCommand {
    Enqueue {
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        token_source: Option<String>,
        #[arg(long)]
        account_id: String,
        #[arg(long)]
        platform: String,
        #[arg(long)]
        action: String,
        #[arg(long, default_value = "{}")]
        params: String,
    },
    Status {
        id: String,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        token_source: Option<String>,
    },
}

#[derive(Args)]
struct SkarbiecArgs {
    #[command(subcommand)]
    command: SkarbiecCommand,
}

#[derive(Subcommand)]
enum SkarbiecCommand {
    Check {
        source: String,
    },
    Reference {
        #[arg(long)]
        item: String,
        #[arg(long)]
        field: String,
        #[arg(long)]
        name: String,
    },
}

#[derive(Args)]
struct BramaArgs {
    #[command(subcommand)]
    command: BramaCommand,
}

#[derive(Subcommand)]
enum BramaCommand {
    Health {
        #[arg(long)]
        base_url: Option<String>,
    },
    Analyze {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long)]
        signing_secret_source: Option<String>,
        #[arg(long, default_value = "task:ugc-review")]
        model: String,
        #[arg(long)]
        instruction: Option<String>,
    },
}

#[derive(Args)]
struct StandaloneArgs {
    #[command(subcommand)]
    command: StandaloneCommand,
}

#[derive(Subcommand)]
enum StandaloneCommand {
    ImportCreators {
        file: PathBuf,
    },
    Discover {
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long, value_delimiter = ',')]
        markets: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        languages: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        niches: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        channels: Vec<String>,
        #[arg(long)]
        min_followers: Option<i64>,
        #[arg(long)]
        max_rate_minor: Option<i64>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Launch {
        #[arg(long)]
        campaign: String,
        #[arg(long)]
        brief: String,
        #[arg(long, value_delimiter = ',')]
        markets: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        languages: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        niches: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        channels: Vec<String>,
        #[arg(long)]
        min_followers: Option<i64>,
        #[arg(long)]
        max_rate_minor: Option<i64>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        offer_minor: Option<i64>,
        #[arg(long)]
        shipping_required: bool,
    },
    ConversationCreate {
        #[arg(long)]
        creator: String,
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long)]
        brief: Option<String>,
        #[arg(long)]
        offer_minor: Option<i64>,
        #[arg(long, default_value = "USD")]
        currency: String,
        #[arg(long)]
        shipping_required: bool,
        #[arg(long)]
        message: Option<String>,
    },
    ConversationList {
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long)]
        creator: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    ConversationMessages {
        id: String,
    },
    ConversationReceive {
        id: String,
        #[arg(long)]
        body: String,
        #[arg(long, default_value = "local_portal")]
        channel: String,
        #[arg(long)]
        external_id: Option<String>,
    },
    ConversationSend {
        id: String,
        #[arg(long)]
        body: String,
        #[arg(long, default_value = "local_portal")]
        channel: String,
        #[arg(long)]
        automated: bool,
    },
    ConversationAccept {
        id: String,
    },
    PortalCreate {
        #[arg(long)]
        creator: String,
        #[arg(long)]
        days: Option<i64>,
    },
    PortalRevoke {
        id: String,
    },
    LedgerFund {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        amount_minor: i64,
        #[arg(long, default_value = "USD")]
        currency: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    LedgerRelease {
        #[arg(long)]
        payment: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    LedgerSettle {
        #[arg(long)]
        payment: String,
        #[arg(long)]
        reference: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    LedgerReverse {
        id: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    LedgerBalance {
        #[arg(long)]
        account: String,
        #[arg(long, default_value = "USD")]
        currency: String,
    },
    LedgerList {
        #[arg(long)]
        assignment: Option<String>,
    },
    PublicationAdd {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        submission: String,
        #[arg(long)]
        asset: String,
        #[arg(long)]
        platform: String,
        #[arg(long)]
        channel: String,
        #[arg(long)]
        territory: Option<String>,
        #[arg(long)]
        post_id: Option<String>,
        #[arg(long)]
        url: String,
        #[arg(long)]
        paid: bool,
        #[arg(long)]
        published_at: Option<String>,
    },
    MetricsCapture {
        #[arg(long)]
        publication: String,
        #[arg(long)]
        views: i64,
        #[arg(long)]
        likes: i64,
        #[arg(long)]
        comments: i64,
        #[arg(long)]
        shares: i64,
        #[arg(long)]
        saves: i64,
        #[arg(long)]
        clicks: i64,
        #[arg(long)]
        conversions: i64,
        #[arg(long)]
        revenue_minor: i64,
        #[arg(long)]
        spend_minor: i64,
        #[arg(long, default_value = "USD")]
        currency: String,
        #[arg(long, default_value = "manual")]
        source: String,
        #[arg(long)]
        captured_at: Option<String>,
    },
    AttributionAdd {
        #[arg(long)]
        publication: String,
        #[arg(long)]
        event_type: String,
        #[arg(long)]
        external_id: Option<String>,
        #[arg(long)]
        value_minor: Option<i64>,
        #[arg(long)]
        currency: Option<String>,
        #[arg(long, default_value = "{}")]
        metadata: String,
        #[arg(long)]
        occurred_at: Option<String>,
    },
    Performance {
        #[arg(long)]
        campaign: String,
    },
    Workflow {
        #[arg(long)]
        campaign: String,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        release_payments: bool,
    },
    Dashboard,
    Serve {
        #[arg(long, default_value = "127.0.0.1:8765")]
        bind: String,
        #[arg(long)]
        operator_token_source: Option<String>,
        #[arg(long)]
        allow_registration: bool,
    },
    Export {
        file: PathBuf,
    },
    Import {
        file: PathBuf,
    },
}

#[derive(Args)]
struct AuditArgs {
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    id: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit("x".len() as i32);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db.unwrap_or_else(default_db_path);
    let asset_dir = cli.asset_dir.unwrap_or_else(default_asset_dir);
    let store = Store::open(&db_path)?;
    let service = UgcService {
        store: &store,
        actor: &cli.actor,
    };
    let standalone = StandaloneService {
        store: &store,
        actor: &cli.actor,
    };

    match cli.command {
        Command::Connection(args) => match args.command {
            ConnectionCommand::Add {
                name,
                provider,
                base_url,
                token_source,
                webhook_secret_source,
                external_account_id,
            } => output(&service.add_connection(
                name,
                provider,
                base_url,
                token_source,
                webhook_secret_source,
                external_account_id,
            )?)?,
            ConnectionCommand::List => {
                output(&store.list::<Connection>("connection", None, None)?)?
            }
            ConnectionCommand::Show { id } => output(&store.get::<Connection>("connection", &id)?)?,
            ConnectionCommand::Health { id } => {
                let connection: Connection = store.get("connection", &id)?;
                output(&provider::adapter(&connection)?.health()?)?;
            }
            ConnectionCommand::Remove { id } => {
                store.delete("connection", &id)?;
                output(&json!({"removed": id}))?;
            }
        },
        Command::Campaign(args) => match args.command {
            CampaignCommand::Create {
                name,
                brand,
                product,
                objective,
                markets,
                languages,
                channels,
                budget_minor,
                currency,
                deadline,
            } => output(&service.create_campaign(
                name,
                brand,
                product,
                objective,
                markets,
                languages,
                channels,
                budget_minor,
                currency,
                deadline,
            )?)?,
            CampaignCommand::List { status } => {
                output(&store.list::<Campaign>("campaign", None, status.as_deref())?)?
            }
            CampaignCommand::Show { id } => output(&store.get::<Campaign>("campaign", &id)?)?,
            CampaignCommand::Status { id, status } => {
                output(&service.campaign_status(&id, &status)?)?
            }
            CampaignCommand::Publish {
                id,
                brief,
                connection,
            } => output(&service.publish_campaign(id, brief, connection)?)?,
            CampaignCommand::Publications { id } => {
                output(&store.list::<Publication>("publication", Some(&id), None)?)?
            }
        },
        Command::Brief(args) => match args.command {
            BriefCommand::Add {
                campaign,
                service_type,
                creative_angle,
                requirements,
                forbidden_claims,
                required_shots,
                talking_points,
                cta,
                duration_min_ms,
                duration_max_ms,
                aspect_ratios,
                raw_footage_required,
                revision_limit,
                rights_requirements,
            } => output(&service.add_brief(
                campaign,
                service_type,
                creative_angle,
                requirements,
                forbidden_claims,
                required_shots,
                talking_points,
                cta,
                duration_min_ms,
                duration_max_ms,
                aspect_ratios,
                raw_footage_required,
                revision_limit,
                parse_json(&rights_requirements)?,
            )?)?,
            BriefCommand::List { campaign } => {
                output(&store.list::<Brief>("brief", Some(&campaign), None)?)?
            }
            BriefCommand::Show { id } => output(&store.get::<Brief>("brief", &id)?)?,
            BriefCommand::Approve { id } => output(&service.approve_brief(&id)?)?,
        },
        Command::Creator(args) => match args.command {
            CreatorCommand::Add {
                name,
                email,
                languages,
                markets,
                niches,
                metadata,
            } => output(&service.add_creator(
                name,
                email,
                languages,
                markets,
                niches,
                parse_json(&metadata)?,
            )?)?,
            CreatorCommand::Verify { id, metadata } => {
                output(&service.verify_creator(&id, parse_json(&metadata)?)?)?
            }
            CreatorCommand::List => output(&store.list::<Creator>("creator", None, None)?)?,
            CreatorCommand::Show { id } => output(&store.get::<Creator>("creator", &id)?)?,
            CreatorCommand::Identity {
                creator,
                connection,
                platform,
                external_id,
                profile_url,
                metadata,
            } => output(&service.add_creator_identity(
                creator,
                connection,
                platform,
                external_id,
                profile_url,
                parse_json(&metadata)?,
            )?)?,
            CreatorCommand::Identities { creator } => {
                output(&store.list::<CreatorIdentity>("creator_identity", Some(&creator), None)?)?
            }
        },
        Command::Assignment(args) => match args.command {
            AssignmentCommand::Create {
                campaign,
                brief,
                creator,
                connection,
                compensation_minor,
                currency,
                payment_owner,
                deadline,
                shipping_required,
                revision_limit,
                external_id,
            } => output(&service.create_assignment(
                campaign,
                brief,
                creator,
                connection,
                compensation_minor,
                currency,
                payment_owner,
                deadline,
                shipping_required,
                revision_limit,
                external_id,
            )?)?,
            AssignmentCommand::List { campaign, status } => output(&store.list::<Assignment>(
                "assignment",
                campaign.as_deref(),
                status.as_deref(),
            )?)?,
            AssignmentCommand::Show { id } => output(&store.get::<Assignment>("assignment", &id)?)?,
            AssignmentCommand::Status { id, status } => {
                output(&service.assignment_status(&id, &status)?)?
            }
        },
        Command::Shipment(args) => match args.command {
            ShipmentCommand::Update {
                assignment,
                status,
                carrier,
                tracking,
                product_variant,
                address_json,
            } => output(
                &service.update_shipment(
                    assignment,
                    status,
                    carrier,
                    tracking,
                    product_variant,
                    address_json
                        .as_deref()
                        .map(serde_json::from_str::<ShippingAddress>)
                        .transpose()?,
                )?,
            )?,
            ShipmentCommand::List { assignment } => {
                output(&store.list::<Shipment>("shipment", Some(&assignment), None)?)?
            }
        },
        Command::Submission(args) => match args.command {
            SubmissionCommand::Add {
                assignment,
                external_id,
            } => output(&service.add_submission(assignment, external_id)?)?,
            SubmissionCommand::List { assignment, status } => output(&store.list::<Submission>(
                "submission",
                assignment.as_deref(),
                status.as_deref(),
            )?)?,
            SubmissionCommand::Show { id } => output(&store.get::<Submission>("submission", &id)?)?,
            SubmissionCommand::Review {
                id,
                status,
                feedback,
            } => output(&service.submission_review(&id, &status, feedback)?)?,
        },
        Command::Asset(args) => match args.command {
            AssetCommand::Import {
                path,
                submission,
                role,
                source_url,
            } => output(&media::import_asset(
                &store,
                &asset_dir,
                &path,
                submission.as_deref(),
                &role,
                source_url,
                &cli.actor,
            )?)?,
            AssetCommand::List { submission } => {
                output(&store.list::<model::Asset>("asset", submission.as_deref(), None)?)?
            }
            AssetCommand::Show { id } => output(&store.get::<model::Asset>("asset", &id)?)?,
            AssetCommand::Qc {
                id,
                mime_prefix,
                min_duration_ms,
                max_duration_ms,
                aspect_ratios,
                max_bytes,
            } => {
                let policy = QcPolicy {
                    expected_mime_prefix: mime_prefix,
                    min_duration_ms,
                    max_duration_ms,
                    allowed_aspect_ratios: aspect_ratios,
                    max_bytes,
                };
                output(&media::run_qc(&store, &id, &policy, &cli.actor)?)?;
            }
        },
        Command::Rights(args) => match args.command {
            RightsCommand::Grant {
                assignment,
                asset,
                owner,
                license_type,
                organic,
                paid_ads,
                whitelisting,
                editing,
                ai_transform,
                raw_footage,
                territories,
                channels,
                starts_at,
                expires_at,
                model_release,
                music_cleared,
                contract_url,
            } => {
                let rights = UsageRights {
                    id: Store::id(),
                    assignment_id: assignment,
                    asset_id: asset,
                    owner,
                    license_type,
                    organic_allowed: organic,
                    paid_ads_allowed: paid_ads,
                    whitelisting_allowed: whitelisting,
                    editing_allowed: editing,
                    ai_transform_allowed: ai_transform,
                    raw_footage_allowed: raw_footage,
                    territories,
                    channels,
                    starts_at: starts_at.unwrap_or_else(Store::now),
                    expires_at,
                    model_release,
                    music_cleared,
                    contract_url,
                    created_at: Store::now(),
                };
                output(&service.grant_rights(rights)?)?;
            }
            RightsCommand::Check {
                assignment,
                asset,
                channel,
                territory,
                paid,
                at,
            } => output(&service.check_rights(
                &assignment,
                asset.as_deref(),
                &channel,
                territory.as_deref(),
                paid,
                at.as_deref(),
            )?)?,
            RightsCommand::List { assignment } => {
                output(&store.list::<UsageRights>("usage_rights", Some(&assignment), None)?)?
            }
        },
        Command::Payment(args) => match args.command {
            PaymentCommand::Create {
                assignment,
                submission,
                amount_minor,
                currency,
                external_id,
            } => output(&service.create_payment(
                assignment,
                submission,
                amount_minor,
                currency,
                external_id,
            )?)?,
            PaymentCommand::List { assignment, status } => output(&store.list::<Payment>(
                "payment",
                assignment.as_deref(),
                status.as_deref(),
            )?)?,
            PaymentCommand::Show { id } => output(&store.get::<Payment>("payment", &id)?)?,
            PaymentCommand::Status {
                id,
                status,
                external_id,
                error,
            } => output(&service.payment_status(&id, &status, external_id, error)?)?,
        },
        Command::Message(args) => match args.command {
            MessageCommand::Send {
                assignment,
                direction,
                channel,
                body,
            } => output(&service.send_message(assignment, direction, channel, body)?)?,
            MessageCommand::List { assignment } => {
                output(&store.list::<Message>("message", Some(&assignment), None)?)?
            }
        },
        Command::Sync(args) => match args.command {
            SyncCommand::Run { limit } => output(&sync::process_outbox(
                &store,
                limit.unwrap_or_else(default_batch),
                &cli.actor,
            )?)?,
            SyncCommand::Connection { id } => {
                output(&sync::sync_connection(&store, &id, &cli.actor)?)?
            }
            SyncCommand::Outbox { status } => output(&store.list_outbox(status.as_deref())?)?,
            SyncCommand::Replay { id } => {
                store.replay_outbox(&id)?;
                output(&json!({"replayed": id}))?;
            }
        },
        Command::Webhook(args) => match args.command {
            WebhookCommand::Ingest {
                connection,
                file,
                signature,
            } => {
                let body = read_input(file.as_deref())?;
                output(&sync::ingest_webhook(
                    &store,
                    &connection,
                    &body,
                    signature.as_deref(),
                    &cli.actor,
                )?)?;
            }
            WebhookCommand::Serve {
                connection,
                bind,
                once,
            } => server::serve(&store, &connection, &bind, &cli.actor, once)?,
            WebhookCommand::Log { connection } => {
                output(&store.webhook_log(connection.as_deref())?)?
            }
        },
        Command::Weles(args) => match args.command {
            WelesCommand::Enqueue {
                base_url,
                token_source,
                account_id,
                platform,
                action,
                params,
            } => {
                let base_url =
                    first_option_or_env(base_url, &["WELES_SUPABASE_URL", "SUPABASE_URL"])?;
                let token_source = option_or_env(token_source, "WELES_TOKEN_SOURCE")?;
                output(&ecosystem::weles_enqueue(
                    &base_url,
                    &token_source,
                    &account_id,
                    &platform,
                    &action,
                    parse_json(&params)?,
                )?)?;
            }
            WelesCommand::Status {
                id,
                base_url,
                token_source,
            } => {
                let base_url =
                    first_option_or_env(base_url, &["WELES_SUPABASE_URL", "SUPABASE_URL"])?;
                let token_source = option_or_env(token_source, "WELES_TOKEN_SOURCE")?;
                output(&ecosystem::weles_status(&base_url, &token_source, &id)?)?;
            }
        },
        Command::Skarbiec(args) => match args.command {
            SkarbiecCommand::Check { source } => {
                secret::check(&source)?;
                output(&json!({"available": true, "source": source}))?;
            }
            SkarbiecCommand::Reference { item, field, name } => {
                output(&json!({
                    "template_line": format!("{name}=skarbiec://{item}/{field}"),
                    "connection_source": format!("file:.ugc/provider.env#{name}")
                }))?;
            }
        },
        Command::Brama(args) => match args.command {
            BramaCommand::Health { base_url } => {
                let base_url = option_or_env(base_url, "BRAMA_URL")?;
                output(&ecosystem::brama_health(&base_url)?)?;
            }
            BramaCommand::Analyze {
                kind,
                id,
                base_url,
                agent_id,
                signing_secret_source,
                model,
                instruction,
            } => {
                let base_url = option_or_env(base_url, "BRAMA_URL")?;
                let agent_id = option_or_env(agent_id, "UGC_BRAMA_AGENT_ID")?;
                let signing_secret_source =
                    option_or_env(signing_secret_source, "UGC_BRAMA_SIGNING_SECRET_SOURCE")?;
                let subject: Value = store.get(&kind, &id)?;
                output(&ecosystem::brama_analyze(
                    &base_url,
                    &agent_id,
                    &signing_secret_source,
                    &model,
                    &kind,
                    &subject,
                    instruction.as_deref(),
                )?)?;
            }
        },
        Command::Standalone(args) => match args.command {
            StandaloneCommand::ImportCreators { file } => {
                let seeds: Vec<CreatorSeed> = serde_json::from_slice(
                    &fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?,
                )
                .context("creator import must be a JSON array")?;
                output(&standalone.import_creators(seeds)?)?;
            }
            StandaloneCommand::Discover {
                campaign,
                markets,
                languages,
                niches,
                channels,
                min_followers,
                max_rate_minor,
                limit,
            } => output(&standalone.discover(DiscoveryQuery {
                campaign_id: campaign,
                markets,
                languages,
                niches,
                channels,
                min_followers,
                max_rate_minor,
                limit,
            })?)?,
            StandaloneCommand::Launch {
                campaign,
                brief,
                markets,
                languages,
                niches,
                channels,
                min_followers,
                max_rate_minor,
                limit,
                offer_minor,
                shipping_required,
            } => output(&standalone.launch_campaign(
                &campaign,
                &brief,
                DiscoveryQuery {
                    campaign_id: Some(campaign.clone()),
                    markets,
                    languages,
                    niches,
                    channels,
                    min_followers,
                    max_rate_minor,
                    limit,
                },
                offer_minor,
                shipping_required,
            )?)?,
            StandaloneCommand::ConversationCreate {
                creator,
                campaign,
                brief,
                offer_minor,
                currency,
                shipping_required,
                message,
            } => output(&standalone.create_conversation(
                creator,
                campaign,
                brief,
                offer_minor,
                currency,
                shipping_required,
                message,
            )?)?,
            StandaloneCommand::ConversationList {
                campaign,
                creator,
                status,
            } => output(&standalone.list_conversations(
                campaign.as_deref(),
                creator.as_deref(),
                status.as_deref(),
            )?)?,
            StandaloneCommand::ConversationMessages { id } => output(&standalone.messages(&id)?)?,
            StandaloneCommand::ConversationReceive {
                id,
                body,
                channel,
                external_id,
            } => output(&standalone.receive_message(&id, body, channel, external_id)?)?,
            StandaloneCommand::ConversationSend {
                id,
                body,
                channel,
                automated,
            } => output(&standalone.send_message(&id, body, channel, automated)?)?,
            StandaloneCommand::ConversationAccept { id } => {
                output(&standalone.accept_conversation(&id)?)?
            }
            StandaloneCommand::PortalCreate { creator, days } => {
                output(&standalone.create_portal_access(&creator, days)?)?
            }
            StandaloneCommand::PortalRevoke { id } => output(&standalone.revoke_portal(&id)?)?,
            StandaloneCommand::LedgerFund {
                assignment,
                amount_minor,
                currency,
                idempotency_key,
            } => output(&standalone.fund_escrow(
                &assignment,
                amount_minor,
                currency,
                idempotency_key.unwrap_or_else(Store::id),
            )?)?,
            StandaloneCommand::LedgerRelease {
                payment,
                idempotency_key,
            } => output(
                &standalone.release_payment(&payment, idempotency_key.unwrap_or_else(Store::id))?,
            )?,
            StandaloneCommand::LedgerSettle {
                payment,
                reference,
                idempotency_key,
            } => output(&standalone.settle_offline(
                &payment,
                reference,
                idempotency_key.unwrap_or_else(Store::id),
            )?)?,
            StandaloneCommand::LedgerReverse {
                id,
                reason,
                idempotency_key,
            } => output(&standalone.reverse_transfer(
                &id,
                reason,
                idempotency_key.unwrap_or_else(Store::id),
            )?)?,
            StandaloneCommand::LedgerBalance { account, currency } => {
                output(&standalone.balance(&account, &currency)?)?
            }
            StandaloneCommand::LedgerList { assignment } => {
                output(&standalone.ledger(assignment.as_deref())?)?
            }
            StandaloneCommand::PublicationAdd {
                assignment,
                submission,
                asset,
                platform,
                channel,
                territory,
                post_id,
                url,
                paid,
                published_at,
            } => output(&standalone.add_publication(
                &assignment,
                &submission,
                &asset,
                platform,
                channel,
                territory,
                post_id,
                url,
                paid,
                published_at,
            )?)?,
            StandaloneCommand::MetricsCapture {
                publication,
                views,
                likes,
                comments,
                shares,
                saves,
                clicks,
                conversions,
                revenue_minor,
                spend_minor,
                currency,
                source,
                captured_at,
            } => output(&standalone.capture_metrics(
                &publication,
                MetricInput {
                    views,
                    likes,
                    comments,
                    shares,
                    saves,
                    clicks,
                    conversions,
                    revenue_minor,
                    spend_minor,
                    currency,
                    source,
                    captured_at,
                },
            )?)?,
            StandaloneCommand::AttributionAdd {
                publication,
                event_type,
                external_id,
                value_minor,
                currency,
                metadata,
                occurred_at,
            } => output(&standalone.add_attribution(
                &publication,
                event_type,
                external_id,
                value_minor,
                currency,
                parse_json(&metadata)?,
                occurred_at,
            )?)?,
            StandaloneCommand::Performance { campaign } => {
                output(&standalone.performance_report(&campaign)?)?
            }
            StandaloneCommand::Workflow {
                campaign,
                apply,
                release_payments,
            } => output(&standalone.advance_workflow(&campaign, apply, release_payments)?)?,
            StandaloneCommand::Dashboard => output(&standalone.dashboard()?)?,
            StandaloneCommand::Serve {
                bind,
                operator_token_source,
                allow_registration,
            } => {
                let operator_token = operator_token_source
                    .as_deref()
                    .map(secret::read)
                    .transpose()?;
                standalone_server::serve(
                    &store,
                    &asset_dir,
                    &bind,
                    &cli.actor,
                    operator_token,
                    allow_registration,
                )?;
            }
            StandaloneCommand::Export { file } => {
                let records = store.all_records()?;
                reject_symbolic_link_output(&file)?;
                fs::write(&file, serde_json::to_vec_pretty(&records)?)
                    .with_context(|| format!("cannot write {}", file.display()))?;
                protect_private_output(&file)?;
                output(&json!({"exported": records.len(), "file": file}))?;
            }
            StandaloneCommand::Import { file } => {
                let records: Vec<Record> = serde_json::from_slice(
                    &fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?,
                )
                .context("standalone import must be a record export JSON array")?;
                output(&store.import_records(&records)?)?;
            }
        },
        Command::Audit(args) => {
            output(&store.audit_log(args.kind.as_deref(), args.id.as_deref())?)?
        }
        Command::Diagnostics => {
            let connections: Vec<Connection> = store.list("connection", None, None)?;
            let health: Vec<Value> = connections.iter().map(|connection| {
                match provider::adapter(connection).and_then(|adapter| adapter.health()) {
                    Ok(result) => json!({"connection_id": connection.id, "healthy": true, "result": result}),
                    Err(error) => json!({"connection_id": connection.id, "healthy": false, "error": error.to_string()}),
                }
            }).collect();
            output(
                &json!({"database": db_path, "asset_dir": asset_dir, "counts": store.counts()?, "connections": health}),
            )?;
        }
    }
    Ok(())
}

fn output<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn parse_json(input: &str) -> Result<Value> {
    serde_json::from_str(input).with_context(|| format!("invalid JSON: {input}"))
}

fn default_db_path() -> PathBuf {
    env::var_os("UGC_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".ugc/ugc.db"))
}

fn default_asset_dir() -> PathBuf {
    env::var_os("UGC_ASSET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".ugc/assets"))
}

fn default_batch() -> usize {
    "batch".len() * "batch".len()
}

fn option_or_env(option: Option<String>, name: &str) -> Result<String> {
    option
        .or_else(|| env::var(name).ok())
        .with_context(|| format!("provide the option or set {name}"))
}

fn first_option_or_env(option: Option<String>, names: &[&str]) -> Result<String> {
    option
        .or_else(|| names.iter().find_map(|name| env::var(name).ok()))
        .with_context(|| format!("provide --base-url or set {}", names.join(" or ")))
}

fn read_input(path: Option<&Path>) -> Result<Vec<u8>> {
    match path {
        Some(path) => fs::read(path).with_context(|| format!("cannot read {}", path.display())),
        None => {
            let mut body = Vec::new();
            io::stdin().read_to_end(&mut body)?;
            Ok(body)
        }
    }
}

fn reject_symbolic_link_output(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("standalone export path must not be a symbolic link")
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("cannot inspect {}", path.display())),
    }
}

#[cfg(unix)]
fn protect_private_output(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = u32::from_str_radix("600", "security".len())?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("cannot protect {}", path.display()))
}

#[cfg(not(unix))]
fn protect_private_output(_path: &Path) -> Result<()> {
    Ok(())
}
