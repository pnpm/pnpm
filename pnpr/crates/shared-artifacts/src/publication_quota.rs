use super::{
    ACTIVE_PUBLICATION_EXPIRY, ArtifactUsage, CompatibilityScopes, Duration,
    MAX_ACTIVE_PUBLICATIONS, PreparedPublication, RegistryError, Result, SystemTime, UNIX_EPOCH,
    compatibility_scopes, storage_quota_error,
};

/// A finish that ran must have updated the usage document.
pub(super) fn finish_outcome(updated: bool) -> Result<()> {
    if updated {
        return Ok(());
    }
    Err(RegistryError::Internal {
        reason: "shared artifact publication finish did not update usage".to_string(),
    })
}

/// Add one publication to the usage document, unless a reclamation is running.
///
/// Already registered is not a fault: a publication written off while it was
/// still working registers again before it looks at what it may have lost, and
/// may find its own registration still there.
pub(super) fn register_publication(usage: &mut ArtifactUsage, publication: &str) -> Result<bool> {
    if usage.reclamation.is_some() {
        return Ok(false);
    }
    if usage.active_publications.len() >= MAX_ACTIVE_PUBLICATIONS {
        return Err(RegistryError::Internal {
            reason: "shared artifact publication concurrency limit reached".to_string(),
        });
    }
    usage.active_publications.insert(publication.to_string());
    usage.active_publication_times.insert(publication.to_string(), registered_now());
    Ok(true)
}

/// The quota one publication reserved, and how much of it it has kept so far.
pub(super) struct PublicationQuota<'a> {
    pub(super) owner: &'a str,
    /// What the reservation charged up front.
    pub(super) added_bytes: u64,
    /// What has since been stored and so stays charged.
    pub(super) retained_bytes: u64,
    pub(super) reclamation_needed: &'a mut bool,
}

/// What a publication is charged before it writes anything: its envelope, the
/// blobs it will store, and the scope markers it is about to claim. The
/// markers are objects like any other, so an owner at their limit cannot write
/// them either.
pub(super) fn publication_charge(
    prepared: &PreparedPublication,
    new_blobs: &[(String, Vec<u8>)],
    envelope_size: u64,
) -> Result<u64> {
    let scopes = match compatibility_scopes(&prepared.payload.compatibility) {
        CompatibilityScopes::Every => 1,
        CompatibilityScopes::These(scopes) => scopes.len(),
    };
    let scope_bytes = (scopes as u64)
        .checked_mul(prepared.envelope_digest.len() as u64)
        .ok_or_else(storage_quota_error)?;
    new_blobs
        .iter()
        .try_fold(envelope_size, |total, entry| {
            total.checked_add(entry.1.len() as u64).ok_or_else(storage_quota_error)
        })?
        .checked_add(scope_bytes)
        .ok_or_else(storage_quota_error)
}

pub(super) fn quota_write_retry_delay(attempt: usize) -> Duration {
    let base = 1_u64 << attempt.min(6);
    let mut random = [0_u8; 1];
    let jitter = if getrandom::fill(&mut random).is_ok() { u64::from(random[0]) % base } else { 0 };
    Duration::from_millis(base + jitter)
}

pub(super) fn registered_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Drops publications that registered longer ago than a publication can
/// plausibly take, reporting whether it changed anything.
///
/// A publication with no registration time was registered by a replica that
/// does not keep them, so it is stamped now rather than written off: it may be
/// in flight, and expiring a live publication lets a collector run beside it.
/// The stamp is what a later pass measures against, which is why the caller
/// persists this whether or not anything was dropped.
pub(super) fn expire_stranded_publications(usage: &mut ArtifactUsage) -> bool {
    let now = registered_now();
    let expiry = now.saturating_sub(ACTIVE_PUBLICATION_EXPIRY.as_secs());
    let before = usage.active_publications.len();
    let times = std::mem::take(&mut usage.active_publication_times);
    let mut stamped = false;
    usage.active_publication_times = usage
        .active_publications
        .iter()
        .map(|publication| {
            let registered = times.get(publication).copied().unwrap_or_else(|| {
                stamped = true;
                now
            });
            (publication.clone(), registered)
        })
        .collect();
    usage
        .active_publications
        .retain(|publication| usage.active_publication_times[publication] > expiry);
    usage
        .active_publication_times
        .retain(|publication, _| usage.active_publications.contains(publication));
    stamped
        || usage.active_publications.len() != before
        || times.len() != usage.active_publication_times.len()
}
