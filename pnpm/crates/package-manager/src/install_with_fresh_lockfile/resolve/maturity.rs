use super::super::InstallWithFreshLockfileError;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};
use pnpm_resolving_npm_resolver::MINIMUM_RELEASE_AGE_VIOLATION_CODE;
use pnpm_resolving_resolver_base::{BlockedVersions, ResolutionPolicyViolation};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::Path,
    sync::Arc,
};

/// Upper bound on resolution passes.
///
/// The loop already terminates on its own — every pass blocks at least one
/// more version, over a finite set — but "finite" is not "small": a package
/// whose every version in range pins something too young would be walked one
/// version per pass, and each pass is a full tree resolution. The bound is
/// what stops that from running for minutes.
///
/// It is set well above the depth any real dependency chain reaches, since
/// blame only climbs one ancestor per pass and a tree deep enough to need more
/// has an unusual number of consecutive exact pins. Hitting it is reported
/// rather than passed over silently — the install then answers with the first
/// pass, and the user has no other way to tell that a later attempt might have
/// found a tree.
const MAX_RESOLUTION_PASSES: usize = 32;

/// Resolve the workspace, backing out of subtrees that no
/// `minimumReleaseAge` cutoff can satisfy.
///
/// The cutoff narrows candidates one packument at a time, so an edge that
/// admits no mature version is a dead end the pick itself cannot escape: a
/// parent that pins its platform bindings to a version whose release was not
/// atomic, or whose newest release depends on a package published minutes
/// ago, has no mature answer to offer. The way out is to pick a different
/// version of whatever declared that edge, and the resolver only reconsiders
/// that on a fresh pass.
///
/// So each pass blocks the immediate parent of every immature pick and
/// resolves again, walking the blame one level up per pass until a tree comes
/// back clean. Passes after the first fetch no registry metadata — the
/// packuments are memoized for the install — and only run while every
/// immature pick still has a parent whose choice could be revisited, so an
/// install that resolves cleanly, or one whose immature picks the manifests
/// ask for by name, pays nothing.
///
/// When no pass comes back clean, the first pass's result is returned: an
/// unavoidable conflict has to report the versions the manifests actually
/// resolve to, not whatever the last attempt happened to reach.
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

/// Record the immediate parent of every immature pick as unusable, and
/// report whether another pass could still reach a clean tree.
///
/// It cannot when a violation has no parent to blame: the importer named
/// that package itself, and no ancestor's choice can widen a range the
/// manifest fixes. Retrying past one of those only re-reaches the same
/// failure, so the install stops here and lets the policy handler act on
/// this pass. It cannot either when every parent to blame is already
/// blocked, which means the walk has run out of ancestors to move.
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
                .entry(name_ver.name.to_string())
                .or_default()
                .insert(name_ver.suffix.to_string());
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
