use pnpm_lockfile::is_tarball_filename;

use super::{
    Arc,
    BTreeMap,
    DEFAULT_CONCURRENCY,
    FuturesUnordered,
    Lockfile,
    LockfileResolution,
    PkgName,
    RenderedViolation,
    ResolutionPolicyViolation,
    ResolutionVerification,
    ResolutionVerifier,
    Semaphore,
    StreamExt,
    VerifyCtx,
    VerifyError,
    file_url_shape::{
        FileShapeContext,
        file_url_matches_target,
    },
    local_containment::roots::normalize_rel_dir,
    local_git_shape::{
        has_local_git_resolution,
        is_local_git_key,
        local_git_key_mismatches_target,
        local_git_key_raw_path,
    },
    offline_checks::{
        has_local_filesystem_resolution,
        is_registry_shaped_resolution,
        strip_file_url_prefix,
    },
};

/// One `(name, version, resolution)` tuple deduplicated from
/// `lockfile.packages`.
pub(super) struct Candidate {
    pub(super) name: PkgName,
    pub(super) version: String,
    pub(super) registry_name: Option<String>,
    pub(super) resolution: LockfileResolution,
}

/// Walk `lockfile.packages` and dedupe by
/// `(name, version, resolution-json)`.
///
/// The serialized resolution is part of the key so two entries that
/// share a `(name, version)` but differ in *what* was resolved (npm
/// vs git URL under the same alias) don't collapse into one.
/// `BTreeMap` over a serialized key gives deterministic iteration
/// order for tests; the fan-out runs across the value iter so order
/// doesn't affect correctness, only the reproducibility of failures.
pub(super) fn collect_candidates(lockfile: &Lockfile) -> Vec<Candidate> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return Vec::new();
    };
    let mut deduped: BTreeMap<String, Candidate> = BTreeMap::new();
    for (key, metadata) in packages {
        let name = key.name.clone();
        // A registry-qualified key (`<name>@<registryName>:<version>`)
        // contributes its bare semver as the candidate version and the
        // alias separately, so the verifier can route by registry while
        // still checking the version against that registry's metadata.
        let registry_name =
            key.suffix.registry_qualified().map(|(registry_name, _)| registry_name.to_string());
        let version = match key.suffix.registry_qualified() {
            Some((_, version)) => version.to_string(),
            None => key.suffix.version().to_string(),
        };
        // Every `LockfileResolution` variant derives `Serialize`, and
        // the wire shape never contains non-string keys or non-finite
        // numbers, the only way this `expect` could fire is a future
        // variant that breaks the contract. Fail loudly rather than
        // skipping the candidate, which would silently bypass
        // verification for that lockfile entry.
        let resolution_json = serde_json::to_string(&metadata.resolution)
            .expect("LockfileResolution must serialize for candidate dedupe");
        let key = format!(
            "{name}@{version}@{}@{resolution_json}",
            registry_name.as_deref().unwrap_or_default(),
        );
        deduped
            .entry(key)
            .or_insert_with(|| Candidate {
                name,
                version,
                registry_name,
                resolution: metadata.resolution.clone(),
            });
    }
    deduped.into_values().collect()
}

/// Check offline invariants between a `packages:` key and its resolution:
/// - a registry-style key must have a registry-shaped resolution;
/// - a local-filesystem resolution (`directory` or `file:` tarball) must have
///   a `file:` key whose workspace containment agrees with its target path;
/// - a local-git resolution must have a matching `git+file:` key.
pub(super) fn shape_mismatch_reason(
    key: &pnpm_lockfile::PkgNameVerPeer,
    resolution: &pnpm_lockfile::LockfileResolution,
    shape_ctx: FileShapeContext<'_>,
) -> Option<&'static str> {
    if has_registry_shape_mismatch(key, resolution) {
        return Some("a registry-style dependency path is backed by a non-registry resolution");
    }
    if !matches!(key.suffix.version(), pnpm_lockfile::VersionPart::File(_))
        && has_local_filesystem_resolution(resolution)
    {
        return Some("a non-file dependency path is backed by a local filesystem resolution");
    }
    if let pnpm_lockfile::VersionPart::File(key_path) = key.suffix.version()
        && file_key_mismatches_local_target(key_path, resolution, shape_ctx)
    {
        return Some("a file dependency path does not match its local resolution target");
    }
    local_git_shape_mismatch_reason(key, resolution, shape_ctx)
}

fn local_git_shape_mismatch_reason(
    key: &pnpm_lockfile::PkgNameVerPeer,
    resolution: &pnpm_lockfile::LockfileResolution,
    shape_ctx: FileShapeContext<'_>,
) -> Option<&'static str> {
    if !is_local_git_key(key) && has_local_git_resolution(resolution) {
        return Some("a non-local-git dependency path is backed by a local git resolution");
    }
    if let Some(key_path) = local_git_key_raw_path(key)
        && local_git_key_mismatches_target(key_path, resolution, shape_ctx)
    {
        return Some("a local git dependency path does not match its git repository target");
    }
    None
}

fn has_registry_shape_mismatch(
    key: &pnpm_lockfile::PkgNameVerPeer,
    resolution: &pnpm_lockfile::LockfileResolution,
) -> bool {
    key.suffix.prefix() == pnpm_lockfile::Prefix::None
        && matches!(
            key.suffix.version(),
            pnpm_lockfile::VersionPart::Semver(_)
                | pnpm_lockfile::VersionPart::RegistryQualified { .. },
        )
        && !is_registry_shaped_resolution(resolution)
}

pub(super) fn file_key_mismatches_local_target(
    key_path: &str,
    resolution: &LockfileResolution,
    shape_ctx: FileShapeContext<'_>,
) -> bool {
    match resolution {
        LockfileResolution::Directory(dir) => {
            if !key_path.starts_with("//") {
                return normalize_rel_dir(key_path) != normalize_rel_dir(&dir.directory);
            }
            is_tarball_filename(key_path)
                || !file_url_matches_target(key_path, &dir.directory, shape_ctx)
        }
        LockfileResolution::Tarball(tarball) => {
            // A remote tarball has its own URL key, so one behind a `file:`
            // key disguises its source.
            let Some(tar_path) = strip_file_url_prefix(&tarball.tarball) else {
                return true;
            };
            if !key_path.starts_with("//") {
                return normalize_rel_dir(key_path) != normalize_rel_dir(tar_path);
            }
            !file_url_matches_target(key_path, tar_path, shape_ctx)
        }
        LockfileResolution::Variations(variations) => variations.variants
            .iter()
            .any(|variant| {
                file_key_mismatches_local_target(key_path, &variant.resolution, shape_ctx)
            }),
        LockfileResolution::Git(_) => true,
        _ => false,
    }
}

/// Run every active verifier against every candidate with a
/// concurrency cap. Each candidate stops at the first verifier that
/// rejects it.
///
/// `on_entry_checked` is called once per candidate as its check
/// completes, and no more once a transport failure aborts the pass.
pub(super) async fn run_fan_out(
    candidates: Vec<Candidate>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    concurrency: Option<usize>,
    on_entry_checked: &mut (dyn FnMut() + Send),
) -> Result<Vec<ResolutionPolicyViolation>, String> {
    let limit = concurrency.unwrap_or(DEFAULT_CONCURRENCY).max(1);
    let semaphore = Arc::new(Semaphore::new(limit));
    let futures = FuturesUnordered::new();
    for candidate in candidates {
        let verifiers = candidate_verifiers(&candidate, verifiers);
        if verifiers.is_empty() {
            on_entry_checked();
            continue;
        }

        let semaphore = Arc::clone(&semaphore);
        futures.push(async move {
            let _permit = semaphore.acquire().await.expect("semaphore not closed during fan-out");
            evaluate_candidate(candidate, &verifiers).await
        });
    }
    drain_fan_out(futures, on_entry_checked).await
}

/// Drain the whole fan-out so no in-flight task is dropped mid-await,
/// keeping only the first abort.
async fn drain_fan_out(
    mut futures: FuturesUnordered<
        impl Future<Output = Result<Option<ResolutionPolicyViolation>, String>>,
    >,
    on_entry_checked: &mut (dyn FnMut() + Send),
) -> Result<Vec<ResolutionPolicyViolation>, String> {
    let mut violations = Vec::new();
    let mut fetch_error: Option<String> = None;
    while let Some(result) = futures.next().await {
        match result {
            Ok(Some(violation)) => violations.push(violation),
            Ok(None) => {}
            Err(message) => {
                if fetch_error.is_none() {
                    fetch_error = Some(message);
                }
            }
        }
        if fetch_error.is_none() {
            on_entry_checked();
        }
    }
    match fetch_error {
        Some(message) => Err(message),
        None => Ok(violations),
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

/// Outcome of evaluating one candidate against the active verifiers.
/// `Err(message)` is a transport failure (the registry couldn't be
/// reached to verify the entry), the runner aborts the whole pass with
/// it rather than collecting it as a policy violation.
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

/// Sort violations by `name@version` and build the matching
/// [`VerifyError`].
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
