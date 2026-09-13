use super::{PatchCandidate, PatchCandidateSet, PatchTarget, PatchTargetError, git_tarball_url};
use node_semver::{Range, Version};
use pnpm_lockfile::Lockfile;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use std::{cmp::Ordering, collections::BTreeSet};

pub fn patch_candidates_from_lockfile(
    raw_dependency: &str,
    current_lockfile: &Lockfile,
) -> Result<PatchCandidateSet, PatchTargetError> {
    let parsed = parse_wanted_dependency(raw_dependency);
    let package_name = parsed.alias
        .as_deref()
        .or(parsed.bare_specifier.as_deref())
        .unwrap_or(raw_dependency);
    let alias = parsed.alias
        .clone()
        .unwrap_or_else(|| raw_dependency.to_string());
    let bare_specifier = parsed.bare_specifier.clone();

    let versions = lockfile_candidates(current_lockfile, package_name);

    let preferred_versions = match bare_specifier.as_deref() {
        Some(specifier) => versions
            .iter()
            .filter(|candidate| version_satisfies(&candidate.version, specifier))
            .cloned()
            .collect(),
        None => versions.clone(),
    };

    if preferred_versions.is_empty() {
        return Err(PatchTargetError::VersionNotFound {
            requested: raw_dependency.to_string(),
            hint: version_not_found_hint(&versions, raw_dependency),
        });
    }

    Ok(PatchCandidateSet {
        alias,
        requested: raw_dependency.to_string(),
        bare_specifier,
        versions,
        preferred_versions,
    })
}

/// Every version of `package_name` the lockfile holds, each once, in
/// version order.
fn lockfile_candidates(current_lockfile: &Lockfile, package_name: &str) -> Vec<PatchCandidate> {
    let mut versions = Vec::new();
    let mut seen = BTreeSet::new();
    for (key, metadata) in current_lockfile.packages
        .as_ref()
        .into_iter()
        .flatten()
    {
        let package_key = key.without_peer();
        let name = package_key.name.to_string();
        if name != package_name {
            continue;
        }
        let version = metadata.version
            .clone()
            .unwrap_or_else(|| package_key.suffix.version().to_string());
        let git_tarball_url = git_tarball_url(&metadata.resolution);
        if seen.insert((name.clone(), version.clone(), git_tarball_url.clone())) {
            versions.push(PatchCandidate {
                name,
                version,
                git_tarball_url,
                package_key,
            });
        }
    }
    versions.sort_by(compare_candidates);
    versions
}

#[must_use]
pub fn default_patch_target(set: &PatchCandidateSet) -> Option<PatchTarget> {
    if set.preferred_versions.len() != 1 {
        return None;
    }
    let preferred = set.preferred_versions.first().expect("len checked");
    let bare_specifier = preferred.git_tarball_url
        .clone()
        .unwrap_or_else(|| preferred.version.clone());
    Some(PatchTarget {
        alias: set.alias.clone(),
        version: preferred.version.clone(),
        bare_specifier,
        apply_to_all: set.bare_specifier.is_none() && preferred.git_tarball_url.is_none(),
        git_tarball_url: preferred.git_tarball_url.clone(),
        package_key: preferred.package_key.clone(),
    })
}

fn version_satisfies(version: &str, range: &str) -> bool {
    let Ok(version) = Version::parse(version) else {
        return false;
    };
    let Ok(range) = Range::parse(range) else {
        return false;
    };
    version.satisfies(&range)
}

pub(super) fn compare_candidates(left: &PatchCandidate, right: &PatchCandidate) -> Ordering {
    match (
        Version::parse(&left.version),
        Version::parse(&right.version),
    ) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => Ordering::Equal,
    }
}

fn version_not_found_hint(versions: &[PatchCandidate], raw_dependency: &str) -> String {
    if versions.is_empty() {
        format!("did you forget to install {raw_dependency}?")
    } else {
        format!(
            "you can specify currently installed version: {}.",
            versions
                .iter()
                .map(|candidate| candidate.version.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}
