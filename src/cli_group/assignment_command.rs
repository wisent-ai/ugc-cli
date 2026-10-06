use super::*;

#[derive(Subcommand)]
pub(crate) enum AssignmentCommand {
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
        #[arg(long, help = "Three-letter currency code the compensation is in")]
        currency: String,
        #[arg(long, help = "Who owns the payment: the operator, the provider, or none")]
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
pub(crate) struct ShipmentArgs {
    #[command(subcommand)]
    pub(crate) command: ShipmentCommand,
}

#[derive(Subcommand)]
pub(crate) enum ShipmentCommand {
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
pub(crate) struct SubmissionArgs {
    #[command(subcommand)]
    pub(crate) command: SubmissionCommand,
}

#[derive(Subcommand)]
pub(crate) enum SubmissionCommand {
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
pub(crate) struct AssetArgs {
    #[command(subcommand)]
    pub(crate) command: AssetCommand,
}

#[derive(Subcommand)]
pub(crate) enum AssetCommand {
    Import {
        path: PathBuf,
        #[arg(long)]
        submission: Option<String>,
        #[arg(long, help = "The imported asset's role in the submission, for example final or draft")]
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
pub(crate) struct RightsArgs {
    #[command(subcommand)]
    pub(crate) command: RightsCommand,
}

#[derive(Subcommand)]
pub(crate) enum RightsCommand {
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
    /// Revoke a grant; it stays as the record of what was licensed and no check counts it any more.
    Revoke {
        id: String,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Args)]
pub(crate) struct PaymentArgs {
    #[command(subcommand)]
    pub(crate) command: PaymentCommand,
}

#[derive(Subcommand)]
pub(crate) enum PaymentCommand {
    Create {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        submission: Option<String>,
        #[arg(long)]
        amount_minor: i64,
        #[arg(long, help = "Three-letter currency code the amount is in")]
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
pub(crate) struct MessageArgs {
    #[command(subcommand)]
    pub(crate) command: MessageCommand,
}

#[derive(Subcommand)]
pub(crate) enum MessageCommand {
    Send {
        #[arg(long)]
        assignment: String,
        #[arg(long, help = "outbound or inbound")]
        direction: String,
        #[arg(long, help = "The channel the message travels on, for example provider or local_portal")]
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
pub(crate) struct SyncArgs {
    #[command(subcommand)]
    pub(crate) command: SyncCommand,
}
