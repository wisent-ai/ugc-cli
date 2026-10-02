use super::*;

#[derive(Parser)]
#[command(
    name = "ugc",
    version,
    about = "Provider-agnostic UGC campaign operations"
)]
pub(crate) struct Cli {
    #[arg(long, global = true, help = "Private asset directory; else UGC_ASSET_DIR. No directory is assumed")]
    pub(crate) asset_dir: Option<PathBuf>,
    #[arg(long, global = true, help = "Who acts, as the audit record names it")]
    pub(crate) actor: String,
    /// Print each JSON answer as indented `path: value` lines for a person.
    /// Without it every command prints its answer as JSON.
    #[arg(long, global = true)]
    pub(crate) text: bool,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    Connection(ConnectionArgs),
    Campaign(CampaignArgs),
    Onboarding(OnboardingArgs),
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
    /// Queue or follow a browser action on a creator platform account (the browser runtime is the adapter).
    Automation(WelesArgs),
    /// Check a credential source, or name a vault reference for one.
    Credential(SkarbiecArgs),
    /// Check the model gateway, or run a model analysis of one record.
    Analysis(BramaArgs),
    Standalone(StandaloneArgs),
    Audit(AuditArgs),
    Diagnostics,
}

#[derive(Args)]
pub(crate) struct OnboardingArgs {
    /// Discard the recorded attempt and replay the journey from its entry screen.
    #[arg(long)]
    pub(crate) reset: bool,
    /// Import a canonical `ugc standalone export` while walking first use.
    #[arg(long, value_name = "EXPORT_JSON")]
    pub(crate) import: Option<PathBuf>,
    /// Emit the whole walk as one JSON document instead of printing screens.
    #[arg(long)]
    pub(crate) json: bool,
    /// Never wait for Enter between screens.
    #[arg(long)]
    pub(crate) yes: bool,
}

#[derive(Args)]
pub(crate) struct ConnectionArgs {
    #[command(subcommand)]
    pub(crate) command: ConnectionCommand,
}

#[derive(Subcommand)]
pub(crate) enum ConnectionCommand {
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long = "token-source")]
        token_source: Option<String>,
        #[arg(long = "webhook-secret-source")]
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
pub(crate) struct CampaignArgs {
    #[command(subcommand)]
    pub(crate) command: CampaignCommand,
}

#[derive(Subcommand)]
pub(crate) enum CampaignCommand {
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
        #[arg(long, help = "Three-letter currency code the budget is in")]
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
    /// Change a campaign's name, objective, deadline or budget; cancel it with `status <id> cancelled`.
    Edit {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        objective: Option<String>,
        #[arg(long)]
        deadline: Option<String>,
        #[arg(long)]
        budget_minor: Option<i64>,
    },
}

#[derive(Args)]
pub(crate) struct BriefArgs {
    #[command(subcommand)]
    pub(crate) command: BriefCommand,
}

#[derive(Subcommand)]
pub(crate) enum BriefCommand {
    Add {
        #[arg(long)]
        campaign: String,
        #[arg(long, help = "The service the brief asks for, for example ugc_content")]
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
    /// Retire a brief: draft or approved goes to archived; assignments keep naming its version.
    Archive {
        id: String,
    },
}

#[derive(Args)]
pub(crate) struct CreatorArgs {
    #[command(subcommand)]
    pub(crate) command: CreatorCommand,
}

#[derive(Subcommand)]
pub(crate) enum CreatorCommand {
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
    /// Remove a creator and their identities; refused while assignments name them.
    Remove {
        id: String,
    },
}

#[derive(Args)]
pub(crate) struct AssignmentArgs {
    #[command(subcommand)]
    pub(crate) command: AssignmentCommand,
}
