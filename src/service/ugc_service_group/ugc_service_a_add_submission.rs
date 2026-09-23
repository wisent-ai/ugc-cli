use super::*;

impl<'a> UgcService<'a> {
    pub fn add_submission(
        &self,
        assignment_id: String,
        external_submission_id: Option<String>,
    ) -> Result<Submission> {
        if let Some(external_id) = external_submission_id.as_deref() {
            if let Some(existing) = self
                .store
                .find_external::<Submission>("submission", external_id)?
            {
                if existing.assignment_id == assignment_id {
                    return Ok(existing);
                }
                bail!("external submission ID was already used for another assignment");
            }
        }
        let assignment: Assignment = self.store.get("assignment", &assignment_id)?;
        if assignment.status != "in_production" {
            bail!("assignment must be in production before submission");
        }
        let existing: Vec<Submission> =
            self.store.list("submission", Some(&assignment_id), None)?;
        let revision = existing
            .iter()
            .map(|submission| submission.revision)
            .max()
            .unwrap_or(0)
            + "r".len() as i64;
        if let Some(limit) = assignment.revision_limit {
            let Some(maximum_revision) = limit.checked_add("r".len() as i64) else {
                bail!("assignment revision limit overflow");
            };
            if revision > maximum_revision {
                bail!("assignment revision limit has been reached");
            }
        }
        let submission = Submission {
            id: Store::id(),
            assignment_id: assignment_id.clone(),
            external_submission_id,
            revision,
            status: "received".into(),
            feedback: None,
            qc_status: None,
            qc_report: None,
            submitted_at: Store::now(),
            reviewed_at: None,
            approved_at: None,
        };
        self.store.put(
            "submission",
            &submission.id,
            Some(&assignment_id),
            None,
            &submission.status,
            submission.external_submission_id.as_deref(),
            &submission,
            &submission.submitted_at,
        )?;
        self.audit(
            "submission",
            &submission.id,
            "received",
            json!({"assignment_id": assignment_id, "revision": revision}),
        )?;
        Ok(submission)
    }
    pub fn submission_review(
        &self,
        id: &str,
        status: &str,
        feedback: Option<String>,
    ) -> Result<Submission> {
        let mut submission: Submission = self.store.get("submission", id)?;
        if status == "approved" && !matches!(submission.qc_status.as_deref(), Some("PASS" | "WARN"))
        {
            bail!("submission cannot be approved before technical QC passes");
        }
        if matches!(status, "revision_requested" | "rejected")
            && feedback
                .as_deref()
                .is_none_or(|feedback| feedback.trim().is_empty())
        {
            bail!("review feedback is required for revision or rejection");
        }
        if status == "revision_requested" {
            let assignment: Assignment = self.store.get("assignment", &submission.assignment_id)?;
            if assignment
                .revision_limit
                .is_some_and(|limit| submission.revision > limit)
            {
                bail!("assignment revision limit has been reached");
            }
        }
        ensure_transition("submission", &submission.status, status)?;
        submission.status = status.into();
        submission.feedback = feedback;
        submission.reviewed_at = Some(Store::now());
        if status == "approved" {
            submission.approved_at = Some(Store::now());
        }
        self.store.put(
            "submission",
            &submission.id,
            Some(&submission.assignment_id),
            None,
            &submission.status,
            submission.external_submission_id.as_deref(),
            &submission,
            &submission.submitted_at,
        )?;
        self.audit(
            "submission",
            id,
            "reviewed",
            json!({"status": status, "feedback": submission.feedback}),
        )?;
        if status == "revision_requested" {
            let assignment: Assignment = self.store.get("assignment", &submission.assignment_id)?;
            self.assignment_status(&assignment.id, "revision_requested")?;
            if let Some(connection) = assignment.connection_id {
                self.store.enqueue(
                    "request_revision",
                    "submission",
                    id,
                    Some(&connection),
                    &serde_json::to_value(&submission)?,
                )?;
            }
        }
        if status == "rejected" {
            self.assignment_status(&submission.assignment_id, "failed")?;
        }
        Ok(submission)
    }
    pub fn grant_rights(&self, rights: UsageRights) -> Result<UsageRights> {
        let _: Assignment = self.store.get("assignment", &rights.assignment_id)?;
        required("rights owner", &rights.owner)?;
        required("license type", &rights.license_type)?;
        let starts_at = DateTime::parse_from_rfc3339(&rights.starts_at)?.with_timezone(&Utc);
        if let Some(expires_at) = &rights.expires_at {
            let expires_at = DateTime::parse_from_rfc3339(expires_at)?.with_timezone(&Utc);
            if expires_at <= starts_at {
                bail!("usage rights expiration must be after start");
            }
        }
        if let Some(asset_id) = &rights.asset_id {
            let asset: Asset = self.store.get("asset", asset_id)?;
            let Some(submission_id) = asset.submission_id else {
                bail!("rights asset must belong to a submission");
            };
            let submission: Submission = self.store.get("submission", &submission_id)?;
            if submission.assignment_id != rights.assignment_id {
                bail!("rights asset does not belong to assignment");
            }
            if submission.status != "approved" {
                bail!("rights asset submission must be approved");
            }
        }
        self.store.put(
            "usage_rights",
            &rights.id,
            Some(&rights.assignment_id),
            rights.asset_id.as_deref(),
            "active",
            None,
            &rights,
            &rights.created_at,
        )?;
        self.audit(
            "assignment",
            &rights.assignment_id,
            "rights_granted",
            serde_json::to_value(&rights)?,
        )?;
        Ok(rights)
    }
}
