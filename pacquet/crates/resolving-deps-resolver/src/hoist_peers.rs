//! Port of pnpm's
//! [`hoistPeers`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/hoistPeers.ts).
//!
//! Two pure functions that turn a "missing peers" picture into a
//! "what to add to the importer's direct deps" map. Used by the
//! orchestrator (`resolve_importer`) inside its hoist loop.

use std::collections::BTreeMap;

use node_semver::{Range, Version};
use pacquet_resolving_resolver_base::{
    PreferredVersions, VersionSelectorEntry, VersionSelectorType,
};

/// One workspace-root dep the loop can satisfy a peer with. Mirrors
/// the slice of upstream's
/// [`PkgAddressOrLink`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L256-L274)
/// that `hoistPeers` reads.
#[derive(Debug, Clone)]
pub struct WorkspaceRootDep {
    /// The slot name in the importer's `node_modules/`.
    pub alias: String,
    /// The package's real name (for npm-alias entries, differs from
    /// the alias).
    pub pkg_name: String,
    /// The specifier pacquet would resolve. `None` for entries with
    /// no normalized form (e.g. linked-from-disk workspace packages
    /// whose spec is a `link:` path); upstream's check treats those
    /// the same as "not a candidate" via the `?.` guard.
    pub normalized_bare_specifier: Option<String>,
}

/// One entry of `missingRequiredPeers`. Mirrors upstream's
/// [`MissingPeerInfo`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L207-L210)
/// slice — pacquet only reads `range`.
#[derive(Debug, Clone)]
pub struct MissingPeerInfo {
    pub range: String,
}

/// Options for [`hoist_peers`]. Mirrors upstream's options bag.
#[derive(Debug)]
pub struct HoistPeersOptions<'a> {
    pub auto_install_peers: bool,
    pub all_preferred_versions: &'a PreferredVersions,
    pub workspace_root_deps: &'a [WorkspaceRootDep],
}

/// Pick a specifier for each missing required peer. Returns a map of
/// `peer_name → specifier` that the caller will add to the importer's
/// wanted deps. Mirrors upstream's
/// [`hoistPeers`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/hoistPeers.ts#L7-L65).
///
/// The decision cascade per peer:
///
/// 1. Workspace root dep with a matching `alias` → its
///    `normalized_bare_specifier`.
/// 2. Workspace root dep with a matching `pkg_name` (tie-break by
///    lex-sort on alias) → its `normalized_bare_specifier`.
/// 3. Entry in `all_preferred_versions`:
///    - exact-version range + a satisfying preferred version → join
///      `[satisfying, ...non-versions]` with `||`.
///    - exact-version range + no satisfying preferred version +
///      `auto_install_peers` → the range itself (resolver fetches
///      from the registry).
///    - non-exact range → highest preferred version overall (for
///      dedup), joined with non-version selectors via `||`.
/// 4. No preferred-version entry + `auto_install_peers` → the range
///    itself.
/// 5. Otherwise → omit (caller leaves the missing peer alone).
#[must_use]
pub fn hoist_peers(
    opts: &HoistPeersOptions<'_>,
    missing_required_peers: &[(String, MissingPeerInfo)],
) -> BTreeMap<String, String> {
    let mut dependencies = BTreeMap::new();
    for (peer_name, info) in missing_required_peers {
        let range = &info.range;

        if let Some(dep) =
            opts.workspace_root_deps.iter().find(|root_dep| &root_dep.alias == peer_name)
            && let Some(spec) = &dep.normalized_bare_specifier
        {
            dependencies.insert(peer_name.clone(), spec.clone());
            continue;
        }

        let mut by_pkg_name: Vec<&WorkspaceRootDep> = opts
            .workspace_root_deps
            .iter()
            .filter(|root_dep| &root_dep.pkg_name == peer_name)
            .collect();
        by_pkg_name.sort_by(|a, b| a.alias.cmp(&b.alias));
        if let Some(dep) = by_pkg_name.first()
            && let Some(spec) = &dep.normalized_bare_specifier
        {
            dependencies.insert(peer_name.clone(), spec.clone());
            continue;
        }

        if let Some(selectors) = opts.all_preferred_versions.get(peer_name) {
            let mut versions: Vec<&str> = Vec::new();
            let mut non_versions: Vec<&str> = Vec::new();
            for (spec, entry) in selectors {
                let spec_type = match entry {
                    VersionSelectorEntry::Plain(t) => *t,
                    VersionSelectorEntry::Weighted(w) => w.selector_type,
                };
                match spec_type {
                    VersionSelectorType::Version => versions.push(spec.as_str()),
                    _ => non_versions.push(spec.as_str()),
                }
            }
            let is_exact_version = range.parse::<Version>().is_ok();
            let satisfying_version =
                if is_exact_version { max_satisfying(&versions, range) } else { None };
            if let Some(satisfying) = satisfying_version {
                let mut parts: Vec<&str> = vec![satisfying];
                parts.extend(non_versions.iter().copied());
                dependencies.insert(peer_name.clone(), parts.join(" || "));
            } else if is_exact_version && opts.auto_install_peers {
                dependencies.insert(peer_name.clone(), range.clone());
            } else {
                let mut parts: Vec<String> = Vec::new();
                if let Some(highest) = max_satisfying_any(&versions) {
                    parts.push(highest.to_string());
                }
                for spec in &non_versions {
                    parts.push((*spec).to_string());
                }
                if !parts.is_empty() {
                    dependencies.insert(peer_name.clone(), parts.join(" || "));
                }
            }
        } else if opts.auto_install_peers {
            dependencies.insert(peer_name.clone(), range.clone());
        }
    }
    dependencies
}

/// Pick an installable version for each missing optional peer, but only
/// when at least one preferred version satisfies *every* recorded range.
/// Returns `peer_name → version`. Mirrors pnpm's
/// [`getHoistableOptionalPeers`](https://github.com/pnpm/pnpm/blob/a1bda24c4f/installing/deps-resolver/src/hoistPeers.ts#L67-L91).
///
/// Version selectors may be plain entries
/// [produced while resolving](https://github.com/pnpm/pnpm/blob/a1bda24c4f/installing/deps-resolver/src/resolveDependencies.ts#L1439-L1444)
/// or weighted entries
/// [seeded from the wanted lockfile](https://github.com/pnpm/pnpm/blob/a1bda24c4f/lockfile/preferred-versions/src/index.ts#L35-L55).
/// Both are eligible so an already locked optional peer is not discarded
/// during re-resolution.
#[must_use]
pub fn get_hoistable_optional_peers(
    all_missing_optional_peers: &BTreeMap<String, Vec<String>>,
    all_preferred_versions: &PreferredVersions,
) -> BTreeMap<String, String> {
    let mut optional_dependencies = BTreeMap::new();
    for (peer_name, ranges) in all_missing_optional_peers {
        let Some(selectors) = all_preferred_versions.get(peer_name) else { continue };
        let mut max_satisfying_version: Option<Version> = None;
        for (version_str, entry) in selectors {
            let selector_type = match entry {
                VersionSelectorEntry::Plain(selector_type) => *selector_type,
                VersionSelectorEntry::Weighted(weighted) => weighted.selector_type,
            };
            if selector_type != VersionSelectorType::Version {
                continue;
            }
            let Ok(version) = version_str.parse::<Version>() else { continue };
            if !ranges.iter().all(|range| {
                range
                    .parse::<Range>()
                    .is_ok_and(|parsed| satisfies_including_prerelease(&parsed, &version))
            }) {
                continue;
            }
            if max_satisfying_version.as_ref().is_none_or(|cur| version > *cur) {
                max_satisfying_version = Some(version);
            }
        }
        if let Some(version) = max_satisfying_version {
            optional_dependencies.insert(peer_name.clone(), version.to_string());
        }
    }
    optional_dependencies
}

/// Highest version from `versions` that satisfies `range`. Returns
/// `None` if no candidate satisfies. Mirrors upstream's
/// `semver.maxSatisfying(versions, range, { includePrerelease: true })`.
fn max_satisfying<'a>(versions: &'a [&'a str], range: &str) -> Option<&'a str> {
    let parsed_range = range.parse::<Range>().ok()?;
    let mut best: Option<(&str, Version)> = None;
    for spec in versions {
        let Ok(parsed_version) = spec.parse::<Version>() else { continue };
        if !satisfies_including_prerelease(&parsed_range, &parsed_version) {
            continue;
        }
        if best.as_ref().is_none_or(|(_, cur)| parsed_version > *cur) {
            best = Some((*spec, parsed_version));
        }
    }
    best.map(|(spec, _)| spec)
}

/// Check whether `version` satisfies `range`, with the
/// `includePrerelease: true` behavior `semver.maxSatisfying` uses in
/// upstream's hoist-peers picker. The default `Range::satisfies` skips
/// prereleases when the range has none of its own (matching strict
/// semver semantics); the retry with the prerelease tag stripped
/// recovers the candidates upstream accepts. Matches the
/// `satisfies_with_prereleases` pattern in the `resolve_peers` module.
pub(crate) fn satisfies_including_prerelease(range: &Range, version: &Version) -> bool {
    if range.satisfies(version) {
        return true;
    }
    if version.pre_release.is_empty() {
        return false;
    }
    let base = Version {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
        pre_release: Vec::new(),
        build: Vec::new(),
    };
    range.satisfies(&base)
}

/// Highest version overall from `versions` (the `*` range that
/// upstream passes to `semver.maxSatisfying`). Returns `None` when
/// no candidate parses as a valid semver.
fn max_satisfying_any<'a>(versions: &'a [&'a str]) -> Option<&'a str> {
    let mut best: Option<(&str, Version)> = None;
    for spec in versions {
        let Ok(v) = spec.parse::<Version>() else { continue };
        if best.as_ref().is_none_or(|(_, cur)| v > *cur) {
            best = Some((*spec, v));
        }
    }
    best.map(|(spec, _)| spec)
}

#[cfg(test)]
mod tests;
