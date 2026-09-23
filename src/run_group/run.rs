use super::*;

pub(crate) fn run() -> Result<()> {
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

    let scope = RunScope {
        store: &store,
        service: &service,
        standalone: &standalone,
        db_path: &db_path,
        asset_dir: &asset_dir,
        actor: &cli.actor,
    };

    match cli.command {
        Command::Connection(args) => run_connection(args.command, scope)?,
        Command::Campaign(args) => run_campaign(args.command, scope)?,
        Command::Onboarding(args) => onboarding::run(
            &db_path,
            &cli.actor,
            &store,
            args.reset,
            args.import.as_deref(),
            args.json,
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
        Command::Weles(args) => run_weles(args.command, scope)?,
        Command::Skarbiec(args) => run_skarbiec(args.command, scope)?,
        Command::Brama(args) => run_router(args.command, scope)?,
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
                &json!({"database": db_path, "asset_dir": asset_dir, "counts": store.counts()?, "connections": health}),
            )?;
        }
    }
    Ok(())
}

/// What every subcommand handler reads: the ledger, the two services, the
/// resolved paths and the acting operator. Only references, so it is copied.
#[derive(Clone, Copy)]
pub(crate) struct RunScope<'a> {
    pub(crate) store: &'a Store,
    pub(crate) service: &'a UgcService<'a>,
    pub(crate) standalone: &'a StandaloneService<'a>,
    pub(crate) db_path: &'a PathBuf,
    pub(crate) asset_dir: &'a PathBuf,
    pub(crate) actor: &'a String,
}
