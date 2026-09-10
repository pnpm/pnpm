use super::{
    APPROVAL_CLAIM_LEASE, AppState, ApprovalClaim, CanonicalPackageName, DocumentWrite, Identity,
    RegistryError, Response, StatusCode, StoredStagedRecord, Value, cleanup_tmp_slots,
    commit_publishes, json, json_response, load_authorized_record, now_iso, report_unrecorded,
    stage_publish, validate_publish_doc,
};
use axum::response::IntoResponse;

/// `POST /-/stage/:id/approve` — claim the record, publish the held document
/// through the regular validate → stage → commit flow, then drop the record.
pub(super) async fn serve_staged_approve(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> Response {
    let stored = match load_authorized_record(state, identity, registry, stage_id).await {
        Ok(stored) => stored,
        Err(err) => return err.into_response(),
    };
    let claim = match claim_for_approval(state, stage_id, stored).await {
        Ok(claim) => claim,
        Err(err) => return err.into_response(),
    };
    match approve_claimed(state, identity, stage_id, &claim).await {
        Ok(response) => response,
        Err(err) => {
            release_approval_claim(state, stage_id, &claim).await;
            err.into_response()
        }
    }
}

/// Take the staged record for this approval, refusing one another approval
/// holds.
pub(super) async fn claim_for_approval(
    state: &AppState,
    stage_id: &str,
    stored: StoredStagedRecord,
) -> Result<ApprovalClaim, RegistryError> {
    if stored.record.approving_since.as_deref().is_some_and(approval_claim_is_live) {
        return Err(RegistryError::StagedApprovalInFlight { stage_id: stage_id.to_string() });
    }
    let mut record = stored.record;
    record.approving_since = Some(now_iso());
    let claimed_bytes = serde_json::to_vec(&record).expect("a staged record serializes");
    let written = state
        .inner
        .storage
        .replace_staged_meta_if_current(stage_id, &stored.bytes, &claimed_bytes)
        .await?;
    match written {
        DocumentWrite::Written => {
            Ok(ApprovalClaim { record, unclaimed_bytes: stored.bytes, claimed_bytes })
        }
        // Something got between the read and the claim. Another approval
        // leaves its claim behind; one that finished, or a rejection, leaves
        // no record at all.
        DocumentWrite::Conflict => match state.inner.storage.read_staged_meta(stage_id).await? {
            Some(_) => {
                Err(RegistryError::StagedApprovalInFlight { stage_id: stage_id.to_string() })
            }
            None => Err(RegistryError::NotFound),
        },
    }
}

/// Whether a claim started at `since` still holds the record.
///
/// A claim from the future is live: replicas time their claims by their own
/// clocks, and one running ahead must not have its claim read as expired by
/// one running behind. An unparsable timestamp is expired instead — a value
/// pnpr cannot read must not be able to hold a stage forever.
pub(super) fn approval_claim_is_live(since: &str) -> bool {
    let Ok(started) = chrono::DateTime::parse_from_rfc3339(since) else {
        return false;
    };
    let Ok(held) = chrono::Utc::now().signed_duration_since(started).to_std() else {
        return true;
    };
    held < APPROVAL_CLAIM_LEASE
}

/// Whether the record this approval claimed is still the one it claimed: a
/// rejection removes it, and a claim the lease handed to another approval is
/// no longer this one's.
pub(super) async fn still_claimed(
    state: &AppState,
    stage_id: &str,
    claim: &ApprovalClaim,
) -> Result<(), RegistryError> {
    match state.inner.storage.read_staged_meta(stage_id).await? {
        Some(stored) if stored == claim.claimed_bytes => Ok(()),
        Some(_) => Err(RegistryError::StagedApprovalInFlight { stage_id: stage_id.to_string() }),
        None => Err(RegistryError::NotFound),
    }
}

/// Put back the record this approval claimed, so a failed approval can be
/// retried without waiting the claim out. Conditional on the claim still
/// standing: an approval that got as far as removing the record has nothing
/// to put back, and a record something else changed is not ours to restore.
pub(super) async fn release_approval_claim(
    state: &AppState,
    stage_id: &str,
    claim: &ApprovalClaim,
) {
    let restored = state
        .inner
        .storage
        .replace_staged_meta_if_current(stage_id, &claim.claimed_bytes, &claim.unclaimed_bytes)
        .await;
    if let Err(err) = restored {
        tracing::warn!(error = %err, stage_id, "failed to release the claim on a staged publish");
    }
}

/// Publish the held document of a record this request holds the claim on.
pub(super) async fn approve_claimed(
    state: &AppState,
    identity: &Identity,
    stage_id: &str,
    claim: &ApprovalClaim,
) -> Result<Response, RegistryError> {
    let record = &claim.record;
    let Some(body) = state.inner.storage.read_staged_body(stage_id).await? else {
        return Err(RegistryError::Io(std::io::Error::other(format!(
            "staged publish {stage_id} has no stored body",
        ))));
    };
    let incoming: Value = serde_json::from_slice(&body).map_err(RegistryError::Json)?;
    let name =
        CanonicalPackageName::parse(&record.package_name, pnpr_package_name::Ecosystem::Npm)?;
    // Re-validate against the registry state of *now*: rules may have
    // changed since staging, and the version may have been published in
    // the meantime (which surfaces as the usual 409).
    let (validated, target) =
        validate_publish_doc(state, identity, record.registry.as_deref(), name, incoming).await?;

    let _packument_guard = state.inner.package_locks.lock(validated.name.as_str()).await;
    let staged = stage_publish(state, validated, &now_iso(), Some(&target.org)).await?;
    // Nothing is visible yet, which is the last moment a rejection can still
    // take the stage back. Past the commit it cannot: the publish is served.
    if let Err(err) = still_claimed(state, stage_id, claim).await {
        cleanup_tmp_slots(staged.slots).await;
        return Err(err);
    }
    let outcome = commit_publishes(state, vec![staged]).await?;
    // Past the commit the stage is spent, whatever the transaction could not
    // record: leaving the record listed would offer an approval that cannot
    // happen again.
    if let Err(err) = state.inner.storage.remove_staged(stage_id).await {
        // The publish is already committed and visible; a failed record
        // cleanup must not report the approval as failed.
        tracing::warn!(error = %err, stage_id, "approved staged publish but its record cleanup failed");
    }
    report_unrecorded(outcome)?;
    Ok(json_response(StatusCode::CREATED, &json!({ "ok": true })))
}
