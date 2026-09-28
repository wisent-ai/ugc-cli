use super::*;

#[allow(unused_variables)]
pub(crate) fn run_connection(command: ConnectionCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_campaign(command: CampaignCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
        } => {
            let campaign = service.create_campaign(
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
            )?;
            output(&campaign)?;
        }
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
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_brief(command: BriefCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_creator(command: CreatorCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_assignment(command: AssignmentCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}
