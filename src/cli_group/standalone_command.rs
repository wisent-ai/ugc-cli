use super::*;

#[derive(Subcommand)]
pub(crate) enum StandaloneCommand {
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
        #[arg(long)]
        portal_days: i64,
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
        #[arg(long, help = "Three-letter currency code the offer is in")]
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
        #[arg(long, help = "The channel the message arrived on, for example local_portal")]
        channel: String,
        #[arg(long)]
        external_id: Option<String>,
    },
    ConversationSend {
        id: String,
        #[arg(long)]
        body: String,
        #[arg(long, help = "The channel the message is sent on, for example local_portal")]
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
        #[arg(long, help = "Three-letter currency code the amount is in")]
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
        #[arg(long, help = "Three-letter currency code of the balance read")]
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
        #[arg(long, help = "Three-letter currency code the figures are in")]
        currency: String,
        #[arg(long, help = "Where the figures come from, for example manual or a provider name")]
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
        /// host:port the portal listens on; no address is assumed.
        #[arg(long)]
        bind: String,
        #[arg(long)]
        operator_token_source: Option<String>,
        #[arg(long)]
        allow_registration: bool,
        #[arg(long)]
        portal_days: Option<i64>,
        #[arg(long)]
        max_header_line_bytes: usize,
        #[arg(long)]
        max_header_count: usize,
        #[arg(long)]
        max_body_bytes: usize,
    },
    Export {
        file: PathBuf,
    },
    Import {
        file: PathBuf,
    },
}

#[derive(Args)]
pub(crate) struct AuditArgs {
    #[arg(long)]
    pub(crate) kind: Option<String>,
    #[arg(long)]
    pub(crate) id: Option<String>,
}

pub(crate) fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit("x".len() as i32);
    }
}
