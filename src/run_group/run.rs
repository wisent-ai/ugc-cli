use super::*;

pub(crate) fn run() -> Result<()> {
    let cli = Cli::parse();
    set_text(cli.text);
    let actor = cli.actor.clone().ok_or_else(|| {
        anyhow::anyhow!("--actor is required: name who acts, as the audit record names it")
    })?;
    let asset_dir = asset_dir(cli.asset_dir)?;
    let store = Store::open()?;
    let service = UgcService {
        store: &store,
        actor: &actor,
    };
    let standalone = StandaloneService {
        store: &store,
        actor: &actor,
    };

    let scope = RunScope {
        store: &store,
        service: &service,
        standalone: &standalone,
        asset_dir: &asset_dir,
        actor: &actor,
    };

    match cli.command {
        Command::Connection(args) => run_connection(args.command, scope)?,
        Command::Campaign(args) => run_campaign(args.command, scope)?,
        Command::Onboarding(args) => onboarding::run(
            &onboarding_dir(&asset_dir),
            &actor,
            &store,
            args.reset,
            args.import.as_deref(),
            cli.json,
            args.yes,
        )?,
        Command::Brief(args) => run_brief(args.command, scope)?,
        Command::Creator(args) => run_creator(args.command, scope)?,
        Command::Assignment(args) => run_assignment(args.command, scope)?,
        Command::Shipment(args) => run_shipment(args.command, scope)?,
        Command::Submission(args) => run_submission(args.command, scope)?,
        Command::Asset(args) => run_asset(args.command, scope)?,
        Command::Rights(args) => run_rights(args.command, scope)?,
        Command::Payment(args) => run_payment(args.command, scope)?,
        Command::Message(args) => run_message(args.command, scope)?,
        Command::Sync(args) => run_sync(args.command, scope)?,
        Command::Webhook(args) => run_webhook(args.command, scope)?,
        Command::Automation(args) => run_weles(args.command, scope)?,
        Command::Credential(args) => run_skarbiec(args.command, scope)?,
        Command::Analysis(args) => run_router(args.command, scope)?,
        Command::Standalone(args) => run_standalone(args.command, scope)?,
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
                &json!({"database": format!("fleet database {}", Store::DATABASE), "asset_dir": asset_dir, "counts": store.counts()?, "connections": health}),
            )?;
        }
    }
    Ok(())
}

/// Where this operator's first-use progress is kept: beside the asset
/// directory, since the ledger itself is the fleet's.
fn onboarding_dir(asset_dir: &std::path::Path) -> PathBuf {
    asset_dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// What every subcommand handler reads: the ledger, the two services, the
/// asset directory and the acting operator. Only references, so it is copied.
#[derive(Clone, Copy)]
pub(crate) struct RunScope<'a> {
    pub(crate) store: &'a Store,
    pub(crate) service: &'a UgcService<'a>,
    pub(crate) standalone: &'a StandaloneService<'a>,
    pub(crate) asset_dir: &'a PathBuf,
    pub(crate) actor: &'a String,
}
