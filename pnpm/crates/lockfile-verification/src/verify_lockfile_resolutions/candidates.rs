use super::{
    Arc, BTreeMap, DEFAULT_CONCURRENCY, FuturesUnordered, Lockfile, LockfileResolution, PkgName,
    RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE, RenderedViolation, ResolutionPolicyViolation,
    ResolutionVerification, ResolutionVerifier, Semaphore, StreamExt, VerifyCtx, VerifyError,
    is_registry_shaped_resolution,
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
pub(super) fn collect_candidates(
    lockfile: &Lockfile,
) -> (Vec<Candidate>, Vec<ResolutionPolicyViolation>) {
    let Some(packages) = lockfile.packages.as_ref() else {
        return (Vec::new(), Vec::new());
    };
    let mut deduped: BTreeMap<String, Candidate> = BTreeMap::new();
    let mut shape_violations = Vec::new();
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
        // A registry-style dep path (`name@semver`, no `runtime:`-style
        // prefix) must be backed by a registry-shaped resolution: the
        // allowBuilds policy derives a trusted package identity from
        // that key shape, which is only sound while this invariant
        // holds. The check is offline, so it applies even when no
        // policy verifiers are active.
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
        // Every `LockfileResolution` variant derives `Serialize`, and
        // the wire shape never contains non-string keys or non-finite
        // numbers — the only way this `expect` could fire is a future
        // variant that breaks the contract. Fail loudly rather than
        // skipping the candidate, which would silently bypass
        // verification for that lockfile entry.
        let resolution_json = serde_json::to_string(&metadata.resolution)
            .expect("LockfileResolution must serialize for candidate dedupe");
        let key = format!(
            "{name}@{version}@{}@{resolution_json}",
            registry_name.as_deref().unwrap_or_default(),
        );
        deduped.entry(key).or_insert_with(|| Candidate {
            name,
            version,
            registry_name,
            resolution: metadata.resolution.clone(),
        });
    }
    (deduped.into_values().collect(), shape_violations)
}

/// A registry-style key must have a registry-shaped resolution because
/// build approval derives the trusted package identity from that key.
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

/// Run every active verifier against every candidate with a
/// concurrency cap. Each candidate stops at the first verifier that
/// rejects it.
///
/// Entries that no active verifier covers count as completed without
/// entering the fan-out. Every completion is reported through
/// `on_entry_checked` with the running count, until a transport
/// failure aborts the pass — the run is incomplete from then on, so
/// reporting stops.
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
        let verifiers = candidate_verifiers(&candidate, verifiers);
        if verifiers.is_empty() {
            checked += 1;
            if let Some(report) = on_entry_checked.as_deref_mut() {
                report(checked);
            }
            continue;
        }

        let semaphore = Arc::clone(&semaphore);
        futures.push(async move {
            // Holding the permit across every verifier .await keeps
            // the effective in-flight count bounded by the semaphore.
            // Releasing per-verifier would let N candidates × M
            // verifiers race past the cap.
            let _permit = semaphore.acquire().await.expect("semaphore not closed during fan-out");
            evaluate_candidate(candidate, &verifiers).await
        });
    }
    // A transport failure (the registry couldn't be reached to verify an
    // entry) aborts the whole pass with the registry's own error rather than
    // collecting it as a policy violation. Drain the rest of the fan-out so no
    // in-flight task is dropped mid-await, but keep only the first abort.
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
            checked += 1;
            if let Some(report) = on_entry_checked.as_deref_mut() {
                report(checked);
            }
        }
    }
    // A registry that couldn't be reached takes precedence over collected
    // violations: the pass never finished, so the batch is incomplete and the
    // actionable failure is the transport error.
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
/// reached to verify the entry) — the runner aborts the whole pass with
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
