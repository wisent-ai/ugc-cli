use super::*;

#[derive(Subcommand)]
pub(crate) enum SyncCommand {
    Run {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        max_attempts: i64,
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
pub(crate) struct WebhookArgs {
    #[command(subcommand)]
    pub(crate) command: WebhookCommand,
}

#[derive(Subcommand)]
pub(crate) enum WebhookCommand {
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
pub(crate) struct WelesArgs {
    #[command(subcommand)]
    pub(crate) command: WelesCommand,
}

#[derive(Subcommand)]
pub(crate) enum WelesCommand {
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
pub(crate) struct SkarbiecArgs {
    #[command(subcommand)]
    pub(crate) command: SkarbiecCommand,
}

#[derive(Subcommand)]
pub(crate) enum SkarbiecCommand {
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
pub(crate) struct BramaArgs {
    #[command(subcommand)]
    pub(crate) command: BramaCommand,
}

#[derive(Subcommand)]
pub(crate) enum BramaCommand {
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
pub(crate) struct StandaloneArgs {
    #[command(subcommand)]
    pub(crate) command: StandaloneCommand,
}
