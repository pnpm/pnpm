use super::super::InstallWithFreshLockfileError;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};
use pnpm_resolving_npm_resolver::MINIMUM_RELEASE_AGE_VIOLATION_CODE;
use pnpm_resolving_resolver_base::{BlockedVersions, ResolutionPolicyViolation};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::Path,
    sync::Arc,
};

// Each pass resolves the whole graph; the cap bounds work on hostile packuments.
const MAX_RESOLUTION_PASSES: usize = 32;

/// Returns a maturity-compliant tree when bounded parent-version retries find one,
/// preserving other policy violations for their handlers. Returns the original
/// tree when no retry succeeds, and propagates resolution errors.
pub(in super::super) async fn resolve_mature_dependency_tree<Reporter, Resolve, Fut>(
    mut resolve: Resolve,
    lockfile_dir: &Path,
    minimum_release_age_active: bool,
) -> Result<pnpm_resolving_deps_resolver::ResolveWorkspaceResult, InstallWithFreshLockfileError>
where
    Reporter: pnpm_reporter::Reporter,
    Resolve: FnMut(Option<Arc<BlockedVersions>>) -> Fut,
    Fut: std::future::Future<
            Output = Result<
                pnpm_resolving_deps_resolver::ResolveWorkspaceResult,
                InstallWithFreshLockfileError,
            >,
        >,
{
    let first_pass = resolve(None).await?;
    if !minimum_release_age_active
        || !has_maturity_violations(&first_pass.merged_tree.policy_violations)
    {
        return Ok(first_pass);
    }

    let mut blocked_versions: BlockedVersions = HashMap::new();
    let mut last_pass: Option<pnpm_resolving_deps_resolver::ResolveWorkspaceResult> = None;
    for _ in 1..MAX_RESOLUTION_PASSES {
        let source = last_pass.as_ref().unwrap_or(&first_pass);
        if !block_dead_end_parents(&source.merged_tree.policy_violations, &mut blocked_versions) {
            return Ok(first_pass);
        }
        let pass = resolve(Some(Arc::new(blocked_versions.clone()))).await?;
        if !has_maturity_violations(&pass.merged_tree.policy_violations) {
            report_held_back_parents::<Reporter>(&blocked_versions, &pass, lockfile_dir);
            return Ok(pass);
        }
        last_pass = Some(pass);
    }
    // Fell out of the loop with ancestors still left to try, so the report
    // below is the first pass's, not a proof that no installable tree exists.
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        prefix: lockfile_dir.display().to_string(),
        message: format!(
            "Stopped after {MAX_RESOLUTION_PASSES} resolution attempts while backing off from \
             versions whose dependencies do not satisfy minimumReleaseAge. The versions reported \
             are the ones the first attempt resolved to; an installable combination may still \
             exist further down their ranges.",
        ),
    }));
    Ok(first_pass)
}

fn block_dead_end_parents(
    violations: &[ResolutionPolicyViolation],
    blocked_versions: &mut BlockedVersions,
) -> bool {
    let mut grew = false;
    for violation in violations {
        if violation.code != MINIMUM_RELEASE_AGE_VIOLATION_CODE {
            continue;
        }
        let Some(parent) = violation.parents.last() else { return false };
        grew |= blocked_versions
            .entry(parent.name.to_string())
            .or_default()
            .insert(parent.suffix.version().to_string());
    }
    grew
}

fn report_held_back_parents<Reporter: pnpm_reporter::Reporter>(
    blocked_versions: &BlockedVersions,
    resolved: &pnpm_resolving_deps_resolver::ResolveWorkspaceResult,
    lockfile_dir: &Path,
) {
    let mut resolved_versions_by_name: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for pkg in resolved.merged_tree.packages.values() {
        if let Some(name_ver) = pkg.result.package.name_ver.as_ref() {
            resolved_versions_by_name
                .entry(
                    pkg.result.package.requested_name
                        .clone()
                        .unwrap_or_else(|| name_ver.name.to_string()),
                )
                .or_default()
                .insert(
                    pkg.result.id
                        .as_str()
                        .parse::<pnpm_lockfile::PackageKey>()
                        .map_or_else(
                            |_| name_ver.suffix.to_string(),
                            |key| key.suffix.version().to_string(),
                        ),
                );
        }
    }
    let lines = held_back_lines(blocked_versions, &resolved_versions_by_name);
    if lines.is_empty() {
        return;
    }
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        prefix: lockfile_dir.display().to_string(),
        message: format!(
            "minimumReleaseAge held back the following versions because a package they \
             depend on is younger than the cutoff:\n{}",
            lines.join("\n"),
        ),
    }));
}

fn held_back_lines(
    blocked_versions: &BlockedVersions,
    resolved_versions_by_name: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<String> {
    // Sorted so the report reads the same across runs: the blocked-version
    // set is keyed by hash, and its iteration order is not stable between them.
    let mut blocked_by_name: Vec<(&String, &HashSet<String>)> = blocked_versions.iter().collect();
    blocked_by_name.sort_by(|left, right| left.0.cmp(right.0));
    let mut lines = Vec::new();
    for (name, versions) in blocked_by_name {
        let resolved_to = resolved_versions_by_name
            .get(name)
            .map(|versions| {
                versions
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            });
        let mut sorted_versions: Vec<&String> = versions.iter().collect();
        sorted_versions.sort();
        for version in sorted_versions {
            match resolved_to.as_deref() {
                Some(resolved_to) => {
                    lines.push(format!("  {name}@{version} (resolved to {resolved_to} instead)"));
                }
                None => lines.push(format!("  {name}@{version}")),
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests;

fn has_maturity_violations(violations: &[ResolutionPolicyViolation]) -> bool {
    violations
        .iter()
        .any(|violation| violation.code == MINIMUM_RELEASE_AGE_VIOLATION_CODE)
}
