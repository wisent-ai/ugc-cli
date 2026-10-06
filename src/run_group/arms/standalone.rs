use super::*;

#[allow(unused_variables)]
pub(crate) fn run_standalone(command: StandaloneCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope {
        store,
        service,
        standalone,
        asset_dir,
        actor,
    } = scope;
    match command {
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
            min_engagement_rate,
            min_response_rate,
        } => output(&standalone.discover(DiscoveryQuery {
            campaign_id: campaign,
            markets,
            languages,
            niches,
            channels,
            min_followers,
            max_rate_minor,
            limit,
            min_engagement_rate,
            min_response_rate,
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
            min_engagement_rate,
            min_response_rate,
            offer_minor,
            shipping_required,
            portal_days,
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
                min_engagement_rate,
                min_response_rate,
            },
            offer_minor,
            shipping_required,
            portal_days,
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
        StandaloneCommand::PortalList { creator } => {
            output(&store.list::<PortalAccess>("portal_access", creator.as_deref(), None)?)?
        }
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
            portal_days,
            max_header_line_bytes,
            max_header_count,
            max_body_bytes,
        } => {
            let operator_token = operator_token_source
                .as_deref()
                .map(secret::read)
                .transpose()?;
            standalone_server::serve(
                &store,
                &asset_dir,
                &bind,
                &actor,
                operator_token,
                allow_registration,
                portal_days,
                standalone_server::ServerLimits {
                    header_line_bytes: max_header_line_bytes,
                    header_count: max_header_count,
                    body_bytes: max_body_bytes,
                },
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
            output(&store.import_records(&records, &actor)?)?;
        }
    }
    Ok(())
}
