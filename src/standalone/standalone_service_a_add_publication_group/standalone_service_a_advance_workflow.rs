use super::*;

impl<'a> StandaloneService<'a> {
    pub fn advance_workflow(
        &self,
        campaign_id: &str,
        apply: bool,
        release_payments: bool,
    ) -> Result<WorkflowReport> {
        let service = self.core();
        let mut campaign: Campaign = self.store.get("campaign", campaign_id)?;
        let briefs: Vec<Brief> = self.store.list("brief", Some(campaign_id), None)?;
        let conversations: Vec<Conversation> =
            self.store.list("conversation", Some(campaign_id), None)?;
        let mut assignments: Vec<Assignment> =
            self.store.list("assignment", Some(campaign_id), None)?;
        let publications: Vec<StandalonePublication> =
            self.store
                .list("standalone_publication", Some(campaign_id), None)?;
        let approved_brief = briefs.iter().any(|brief| brief.status == "approved");
        let mut actions = Vec::new();
        let mut blockers = Vec::new();

        self.advance_campaign_phase(
            &service,
            campaign_id,
            &mut campaign,
            approved_brief,
            &conversations,
            &assignments,
            apply,
            &mut actions,
            &mut blockers,
        )?;

        for assignment in &mut assignments {
            let shipments: Vec<Shipment> =
                self.store.list("shipment", Some(&assignment.id), None)?;
            if assignment.status == "accepted" {
                if assignment.shipping_required {
                    if shipments.iter().any(|shipment| {
                        matches!(
                            shipment.status.as_str(),
                            "ready_to_ship" | "shipped" | "delivered"
                        )
                    }) {
                        actions.push(format!(
                            "assignment {} accepted -> product_shipping",
                            assignment.id
                        ));
                        if apply {
                            *assignment =
                                service.assignment_status(&assignment.id, "product_shipping")?;
                        }
                    } else {
                        blockers.push(format!(
                            "collect shipping address for assignment {}",
                            assignment.id
                        ));
                    }
                } else {
                    actions.push(format!(
                        "assignment {} accepted -> in_production",
                        assignment.id
                    ));
                    if apply {
                        *assignment = service.assignment_status(&assignment.id, "in_production")?;
                    }
                }
            }
            if assignment.status == "product_shipping" {
                if shipments
                    .iter()
                    .any(|shipment| shipment.status == "delivered")
                {
                    actions.push(format!(
                        "assignment {} product_shipping -> in_production",
                        assignment.id
                    ));
                    if apply {
                        *assignment = service.assignment_status(&assignment.id, "in_production")?;
                    }
                } else {
                    blockers.push(format!(
                        "deliver product shipment for assignment {}",
                        assignment.id
                    ));
                }
            }
            let submissions: Vec<Submission> =
                self.store.list("submission", Some(&assignment.id), None)?;
            let approved_submission = submissions
                .iter()
                .find(|submission| submission.status == "approved");
            if assignment.status == "in_production"
                && submissions
                    .iter()
                    .all(|submission| submission.status == "rejected")
            {
                blockers.push(format!("submit media for assignment {}", assignment.id));
            }
            if submissions
                .iter()
                .any(|submission| submission.status == "received")
            {
                blockers.push(format!("run QC for assignment {}", assignment.id));
            }
            if submissions
                .iter()
                .any(|submission| submission.status == "pending_review")
            {
                blockers.push(format!(
                    "review submission for assignment {}",
                    assignment.id
                ));
            }
            if assignment.status == "revision_requested" {
                blockers.push(format!(
                    "submit a revised asset for assignment {}",
                    assignment.id
                ));
            }
            if assignment.status == "submitted" && approved_submission.is_some() {
                actions.push(format!(
                    "assignment {} submitted -> approved",
                    assignment.id
                ));
                if apply {
                    *assignment = service.assignment_status(&assignment.id, "approved")?;
                }
            }
            let rights: Vec<UsageRights> =
                self.store
                    .list("usage_rights", Some(&assignment.id), None)?;
            let approved_assets: Vec<Asset> = match approved_submission {
                Some(submission) => self.store.list("asset", Some(&submission.id), None)?,
                None => Vec::new(),
            };
            let applicable_rights: Vec<&UsageRights> = rights
                .iter()
                .filter(|rights| {
                    rights.asset_id.is_none()
                        || approved_assets
                            .iter()
                            .any(|asset| rights.asset_id.as_deref() == Some(&asset.id))
                })
                .collect();
            let rights_ready = !applicable_rights.is_empty()
                && applicable_rights
                    .iter()
                    .all(|rights| rights.model_release && rights.music_cleared);
            if assignment.status == "approved" && rights_ready {
                actions.push(format!("assignment {} approved -> licensed", assignment.id));
                if apply {
                    *assignment = service.assignment_status(&assignment.id, "licensed")?;
                }
            } else if assignment.status == "approved" {
                blockers.push(format!(
                    "record complete rights for assignment {} approved submission",
                    assignment.id
                ));
            }
            let mut payments: Vec<Payment> =
                self.store.list("payment", Some(&assignment.id), None)?;
            if assignment.status == "licensed" && payments.is_empty() {
                if let (Some(submission), Some(amount)) =
                    (approved_submission, assignment.compensation_minor)
                {
                    actions.push(format!("create payment for assignment {}", assignment.id));
                    if apply {
                        let payment = service.create_payment(
                            assignment.id.clone(),
                            Some(submission.id.clone()),
                            amount,
                            assignment.currency.clone(),
                            None,
                        )?;
                        payments.push(payment);
                    }
                } else {
                    blockers.push(format!(
                        "assignment {} lacks approved submission or compensation",
                        assignment.id
                    ));
                }
            }
            if assignment.status == "licensed" && release_payments {
                for payment in payments
                    .iter()
                    .filter(|payment| payment.status == "pending")
                {
                    let escrow = format!("escrow:{}", assignment.id);
                    let balance = self.balance(&escrow, &payment.currency)?.balance_minor;
                    if balance >= payment.amount_minor {
                        actions.push(format!("release payment {} from escrow", payment.id));
                        if apply {
                            self.release_payment(
                                &payment.id,
                                format!("workflow-release:{}", payment.id),
                            )?;
                        }
                    } else {
                        blockers.push(format!("fund escrow for payment {}", payment.id));
                    }
                }
                if apply {
                    payments = self.store.list("payment", Some(&assignment.id), None)?;
                }
            }
            if assignment.status == "licensed"
                && payments.iter().any(|payment| payment.status == "paid")
            {
                actions.push(format!("assignment {} licensed -> paid", assignment.id));
                if apply {
                    *assignment = service.assignment_status(&assignment.id, "paid")?;
                }
            } else if assignment.status == "licensed" {
                if payments
                    .iter()
                    .any(|payment| payment.status == "processing")
                {
                    blockers.push(format!(
                        "record offline settlement for assignment {}",
                        assignment.id
                    ));
                } else {
                    blockers.push(format!(
                        "fund and release payment for assignment {}",
                        assignment.id
                    ));
                }
            }
            if assignment.status == "paid" {
                actions.push(format!("assignment {} paid -> completed", assignment.id));
                if apply {
                    *assignment = service.assignment_status(&assignment.id, "completed")?;
                }
            }
        }

        if campaign.status == "active"
            && !assignments.is_empty()
            && assignments
                .iter()
                .all(|assignment| matches!(assignment.status.as_str(), "completed" | "cancelled"))
        {
            if publications.is_empty() {
                blockers.push("record at least one publication before campaign completion".into());
            } else {
                actions.push("campaign active -> completed".into());
                if apply {
                    campaign = service.campaign_status(campaign_id, "completed")?;
                }
            }
        }
        if conversations.is_empty() {
            blockers.push("no creator conversations".into());
        }
        if assignments.is_empty() {
            blockers.push("no creator assignments".into());
        }
        let report = WorkflowReport {
            campaign_id: campaign_id.into(),
            status: campaign.status,
            blockers: unique(blockers),
            actions: unique(actions),
            counts: json!({
                "briefs": briefs.len(), "conversations": conversations.len(),
                "assignments": assignments.len(), "publications": publications.len()
            }),
            updated_at: Store::now(),
        };
        self.audit(
            "campaign",
            campaign_id,
            if apply {
                "workflow_advanced"
            } else {
                "workflow_inspected"
            },
            serde_json::to_value(&report)?,
        )?;
        Ok(report)
    }
}
