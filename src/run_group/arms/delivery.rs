use super::*;

#[allow(unused_variables)]
pub(crate) fn run_shipment(command: ShipmentCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_submission(command: SubmissionCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_asset(command: AssetCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
            &actor,
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
            output(&media::run_qc(&store, &id, &policy, &actor)?)?;
        }
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_rights(command: RightsCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
        RightsCommand::Revoke { id, reason } => output(&service.revoke_rights(&id, &reason)?)?,
    }
    Ok(())
}

#[allow(unused_variables)]
pub(crate) fn run_payment(command: PaymentCommand, scope: RunScope<'_>) -> Result<()> {
    let RunScope { store, service, standalone, asset_dir, actor } = scope;
    match command {
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
    }
    Ok(())
}
