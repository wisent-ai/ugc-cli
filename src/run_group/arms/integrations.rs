use super::*;

#[allow(unused_variables)]
pub(crate) fn run_message(command: MessageCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
        MessageCommand::Send {
            assignment,
            direction,
            channel,
            body,
        } => output(&service.send_message(assignment, direction, channel, body)?)?,
        MessageCommand::List { assignment } => {
            output(&store.list::<Message>("message", Some(&assignment), None)?)?
        }
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_sync(command: SyncCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
        SyncCommand::Run {
            limit,
            max_attempts,
        } => output(&sync::process_outbox(
            &store,
            limit.unwrap_or(usize::MAX),
            max_attempts,
            &actor,
        )?)?,
        SyncCommand::Connection { id } => {
            output(&sync::sync_connection(&store, &id, &actor)?)?
        }
        SyncCommand::Outbox { status } => output(&store.list_outbox(status.as_deref())?)?,
        SyncCommand::Replay { id } => {
            store.replay_outbox(&id)?;
            output(&json!({"replayed": id}))?;
        }
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_webhook(command: WebhookCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
                &actor,
            )?)?;
        }
        WebhookCommand::Serve {
            connection,
            bind,
            once,
        } => server::serve(&store, &connection, &bind, &actor, once)?,
        WebhookCommand::Log { connection } => {
            output(&store.webhook_log(connection.as_deref())?)?
        }
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_weles(command: WelesCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_skarbiec(command: SkarbiecCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_router(command: BramaCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}
