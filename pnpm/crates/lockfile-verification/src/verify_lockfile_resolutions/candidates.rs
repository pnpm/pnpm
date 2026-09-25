use std::{collections::BTreeMap, sync::Arc};

use futures_util::{StreamExt, stream::FuturesUnordered};
use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageMetadata, PkgName, PkgNameVerPeer, Prefix, VersionPart,
    is_git_hosted_tarball_url,
};
use pnpm_resolving_resolver_base::{
    ResolutionPolicyViolation, ResolutionVerification, ResolutionVerifier, VerifyCtx,
};
use tokio::sync::Semaphore;

use crate::errors::{RenderedViolation, VerifyError};

use super::{DEFAULT_CONCURRENCY, ReplacedEntries};

pub const RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE: &str = "RESOLUTION_SHAPE_MISMATCH";

/// One `(name, version, resolution)` tuple deduplicated from
/// `lockfile.packages`.
pub(super) struct Candidate {
    pub(super) name: PkgName,
    pub(super) version: String,
    pub(super) registry_name: Option<String>,
    pub(super) resolution: LockfileResolution,
}

/// The lockfile entries the policy verifiers check, after the offline
/// shape check passes. Entries `replaced` matches are left out.
pub(super) fn collect_candidates_to_verify(
    lockfile: &Lockfile,
    replaced: Option<ReplacedEntries<'_>>,
) -> Result<(Vec<Candidate>, bool), VerifyError> {
    let (mut candidates, shape_violations) = collect_candidates(lockfile);
    if !shape_violations.is_empty() {
        return Err(build_verification_error(shape_violations));
    }
    let Some(ReplacedEntries(is_replaced)) = replaced else { return Ok((candidates, false)) };
    let entries = candidates.len();
    candidates.retain(|candidate| !is_replaced(&candidate.name, &candidate.version));
    let skipped_replaced = candidates.len() < entries;
    Ok((candidates, skipped_replaced))
}

/// Walk `lockfile.packages` and dedupe by
/// `(name, version, resolution-json)`.
pub(super) fn collect_candidates(
    lockfile: &Lockfile,
) -> (Vec<Candidate>, Vec<ResolutionPolicyViolation>) {
    let Some(packages) = lockfile.packages.as_ref() else {
        return (Vec::new(), Vec::new());
    };
    let mut deduped: BTreeMap<String, Candidate> = BTreeMap::new();
    let mut shape_violations = Vec::new();
    for (key, metadata) in packages {
        insert_candidate(key, metadata, &mut deduped, &mut shape_violations);
    }
    (deduped.into_values().collect(), shape_violations)
}

fn insert_candidate(
    key: &PkgNameVerPeer,
    metadata: &PackageMetadata,
    deduped: &mut BTreeMap<String, Candidate>,
    shape_violations: &mut Vec<ResolutionPolicyViolation>,
) {
    let name = key.name.clone();
    let registry_name =
        key.suffix.registry_qualified().map(|(registry_name, _)| registry_name.to_string());
    let version = match key.suffix.registry_qualified() {
        Some((_, version)) => version.to_string(),
        None => key.suffix.version().to_string(),
    };
    if has_registry_shape_mismatch(key, &metadata.resolution) {
        shape_violations.push(ResolutionPolicyViolation {
            name: name.clone(),
            version: version.clone(),
            resolution: metadata.resolution.clone(),
            code: RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
            reason: "a registry-style dependency path is backed by a non-registry resolution"
                .to_string(),
        });
    }
    let resolution_json = serde_json::to_string(&metadata.resolution)
        .expect("LockfileResolution must serialize for candidate dedupe");
    let map_key = format!(
        "{name}@{version}@{}@{resolution_json}",
        registry_name.as_deref().unwrap_or_default(),
    );
    deduped
        .entry(map_key)
        .or_insert_with(|| Candidate {
            name,
            version,
            registry_name,
            resolution: metadata.resolution.clone(),
        });
}

fn has_registry_shape_mismatch(key: &PkgNameVerPeer, resolution: &LockfileResolution) -> bool {
    key.suffix.prefix() == Prefix::None
        && matches!(
            key.suffix.version(),
            VersionPart::Semver(_) | VersionPart::RegistryQualified { .. },
        )
        && !is_registry_shaped_resolution(resolution)
}

fn is_registry_shaped_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Registry(_) => true,
        LockfileResolution::Tarball(tarball) => {
            is_http_tarball_url(&tarball.tarball)
                && tarball.git_hosted != Some(true)
                && !is_git_hosted_tarball_url(&tarball.tarball)
        }
        LockfileResolution::Variations(variations) => variations.variants
            .iter()
            .all(|variant| is_registry_shaped_resolution(&variant.resolution)),
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Custom(_) => false,
    }
}

fn is_http_tarball_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// Run every active verifier against every candidate with a
/// concurrency cap.
pub(super) async fn run_fan_out(
    candidates: Vec<Candidate>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
    mut on_entry_checked: Option<&mut (dyn FnMut(u64) + Send)>,
) -> Result<Vec<ResolutionPolicyViolation>, String> {
    let limit = concurrency.unwrap_or(DEFAULT_CONCURRENCY).max(1);
    let semaphore = Arc::new(Semaphore::new(limit));
    let mut futures = FuturesUnordered::new();
    let mut checked: u64 = 0;
    for candidate in candidates {
        let matching_verifiers = candidate_verifiers(&candidate, verifiers);
        if matching_verifiers.is_empty() {
            checked += 1;
            report_completion(&mut on_entry_checked, checked);
            continue;
        }

        let semaphore = Arc::clone(&semaphore);
        futures.push(async move {
            let _permit = semaphore.acquire().await.expect("semaphore not closed during fan-out");
            evaluate_candidate(candidate, &matching_verifiers).await
        });
    }
    let (violations, fetch_error) =
        drain_fan_out(&mut futures, &mut checked, &mut on_entry_checked).await;

    fetch_error.map_or(Ok(violations), Err)
}

async fn drain_fan_out<Fut>(
    futures: &mut FuturesUnordered<Fut>,
    checked: &mut u64,
    on_entry_checked: &mut Option<&mut (dyn FnMut(u64) + Send)>,
) -> (Vec<ResolutionPolicyViolation>, Option<String>)
where
    Fut: Future<Output = Result<Option<ResolutionPolicyViolation>, String>>,
{
    let mut violations = Vec::new();
    let mut fetch_error: Option<String> = None;
    while let Some(result) = futures.next().await {
        match result {
            Ok(Some(violation)) => violations.push(violation),
            Ok(None) => {}
            Err(message) => {
                fetch_error.get_or_insert(message);
            }
        }
        if fetch_error.is_none() {
            *checked += 1;
            report_completion(on_entry_checked, *checked);
        }
    }
    (violations, fetch_error)
}

fn report_completion(on_entry_checked: &mut Option<&mut (dyn FnMut(u64) + Send)>, checked: u64) {
    if let Some(report) = on_entry_checked.as_deref_mut() {
        report(checked);
    }
}

fn candidate_verifiers(
    candidate: &Candidate,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Vec<Arc<dyn ResolutionVerifier>> {
    verifiers
        .iter()
        .filter_map(|verifier| {
            let ctx = VerifyCtx {
                name: &candidate.name,
                version: &candidate.version,
                registry_name: candidate.registry_name.as_deref(),
            };
            verifier.might_verify(&candidate.resolution, ctx).then(|| Arc::clone(verifier))
        })
        .collect()
}

async fn evaluate_candidate(
    candidate: Candidate,
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Result<Option<ResolutionPolicyViolation>, String> {
    for verifier in verifiers {
        let ctx = VerifyCtx {
            name: &candidate.name,
            version: &candidate.version,
            registry_name: candidate.registry_name.as_deref(),
        };
        match verifier.verify(&candidate.resolution, ctx).await {
            ResolutionVerification::Ok => continue,
            ResolutionVerification::Err { code, reason } => {
                return Ok(Some(ResolutionPolicyViolation {
                    name: candidate.name,
                    version: candidate.version,
                    resolution: candidate.resolution,
                    code,
                    reason,
                }));
            }
            ResolutionVerification::FetchFailed { message } => return Err(message),
        }
    }
    Ok(None)
}

/// Sort violations by `name@version` and build the matching [`VerifyError`].
pub(super) fn build_verification_error(
    mut violations: Vec<ResolutionPolicyViolation>,
) -> VerifyError {
    violations.sort_by(|left, right| {
        format!("{}@{}", left.name, left.version).cmp(&format!("{}@{}", right.name, right.version))
    });
    let rendered: Vec<RenderedViolation> = violations
        .into_iter()
        .map(|violation| RenderedViolation {
            name: violation.name.to_string(),
            version: violation.version,
            code: violation.code,
            reason: violation.reason,
        })
        .collect();
    VerifyError::from_rendered(&rendered)
}
