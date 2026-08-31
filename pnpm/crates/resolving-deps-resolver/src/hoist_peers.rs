//! Two pure functions that turn a "missing peers" picture into a
//! "what to add to the importer's direct deps" map. Used by the
//! orchestrator (`resolve_importer`) inside its hoist loop.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, path::Path};

use node_semver::{Range, Version};
use pnpm_resolving_resolver_base::{
    PreferredVersions, VersionSelectorEntry, VersionSelectorType, get_peer_version_range,
};
use pnpm_semver_include_prerelease::IncludePrereleaseRange;

/// One workspace-root dep the loop can satisfy a peer with.
#[derive(Debug, Clone)]
pub struct WorkspaceRootDep {
    /// The slot name in the importer's `node_modules/`.
    pub alias: String,
    /// The package's real name (for npm-alias entries, differs from
    /// the alias).
    pub pkg_name: String,
    /// The specifier pacquet would resolve. `None` for entries with
    /// no normalized form (e.g. linked-from-disk workspace packages
    /// whose spec is a `link:` path); those are treated the same as
    /// "not a candidate".
    pub normalized_bare_specifier: Option<String>,
}

/// One entry of `missingRequiredPeers`. Only `range` is read.
#[derive(Debug, Clone)]
pub struct MissingPeerInfo {
    pub range: String,
}

/// Applies `overrides` to a peer nobody declares as a dependency.
/// Such a peer has no manifest for the override hook to rewrite, so
/// without this it would resolve against its declared peer range and
/// silently produce the second copy the override exists to prevent.
/// Returns the overriding specifier, `"-"` when the override drops the
/// peer, or `None` when no override claims it. The path is the directory
/// of the importer the peer is hoisted into — the manifest that would
/// have declared the dependency — which a `link:` / `file:` override
/// target is made relative to.
pub type DependencyOverrider = dyn Fn(&str, &str, &Path) -> Option<String> + Send + Sync;

/// Options for [`hoist_peers`].
pub struct HoistPeersOptions<'a> {
    pub auto_install_peers: bool,
    pub all_preferred_versions: &'a PreferredVersions,
    pub workspace_root_deps: &'a [WorkspaceRootDep],
    pub override_bare_specifier: Option<&'a DependencyOverrider>,
    /// Directory of the importer the peers are hoisted into. Only read
    /// to resolve a local override target; see [`DependencyOverrider`].
    pub project_dir: &'a Path,
}

/// Pick a specifier for each missing required peer. Returns a map of
/// `peer_name → specifier` that the caller will add to the importer's
/// wanted deps.
#[must_use]
pub fn hoist_peers(
    opts: &HoistPeersOptions<'_>,
    missing_required_peers: &[(String, MissingPeerInfo)],
) -> BTreeMap<String, String> {
    let mut dependencies = BTreeMap::new();
    for (peer_name, info) in missing_required_peers {
        let range = &info.range;

        let root_bare_specifier = find_workspace_root_dep(opts.workspace_root_deps, peer_name)
            .and_then(|dep| dep.normalized_bare_specifier.as_ref());
        // An override redirects a hoist; it must never create one, or
        // disabling auto-install-peers would still install a peer nobody
        // depends on. Only the workspace root's own dependency hoists a
        // peer that auto-install-peers is not asking for, so that is the
        // one hoist an override still governs here; the deduplication
        // below installs nothing new either way.
        let overrider = (opts.auto_install_peers || root_bare_specifier.is_some())
            .then_some(opts.override_bare_specifier)
            .flatten();
        if let Some(overridden) =
            overrider.and_then(|overrider| overrider(peer_name, range, opts.project_dir))
        {
            if overridden != "-" {
                dependencies.insert(peer_name.clone(), overridden);
            }
            continue;
        }

        if let Some(spec) = root_bare_specifier {
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
            // Dedupe onto a preferred version only when it actually satisfies
            // the wanted peer range. Picking the highest preferred version
            // regardless of the range lets a version resolved for one importer
            // be auto-installed as another importer's peer even though nothing
            // in that importer's closure accepts it, silently producing a peer
            // graph that mixes incompatible majors. Scheme specifiers
            // (named-registry, npm: aliases, workspace:) contribute a comparable
            // range through get_peer_version_range, so they get range-aware
            // selection too; specs with no version body (catalog:, dist-tags)
            // yield a non-semver value and keep the dedupe-to-highest behavior.
            // The raw scheme is preserved below so the fallback still selects
            // the package to install.
            let range_for_match = get_peer_version_range(range);
            let is_semver_range = range_for_match.parse::<Range>().is_ok();
            let satisfying_version =
                if is_semver_range { max_satisfying(&versions, &range_for_match) } else { None };
            if let Some(satisfying) = satisfying_version {
                let mut parts: Vec<&str> = vec![satisfying];
                parts.extend(non_versions.iter().copied());
                dependencies.insert(peer_name.clone(), parts.join(" || "));
            } else if is_semver_range && !versions.is_empty() {
                // Preferred versions exist but none satisfies the wanted
                // range. Use the range directly so it resolves from the
                // registry rather than installing a version the peer
                // explicitly rejects. Without auto-install-peers, hoist
                // nothing and leave the peer missing.
                if opts.auto_install_peers {
                    dependencies.insert(peer_name.clone(), range.clone());
                }
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
/// when at least one preferred version satisfies *every* recorded range
/// under strict semver. Returns `peer_name → version`.
///
/// Version selectors may be plain entries produced while resolving or
/// weighted entries seeded from the wanted lockfile. Both are eligible
/// so an already locked optional peer is not discarded during
/// re-resolution.
#[must_use]
pub fn get_hoistable_optional_peers(
    all_missing_optional_peers: &BTreeMap<String, Vec<String>>,
    all_preferred_versions: &PreferredVersions,
    workspace_root_deps: &[WorkspaceRootDep],
) -> BTreeMap<String, String> {
    get_hoistable_optional_peers_with_locked_versions(
        all_missing_optional_peers,
        all_preferred_versions,
        workspace_root_deps,
        &HashMap::default(),
    )
}

pub(crate) fn get_hoistable_optional_peers_with_locked_versions(
    all_missing_optional_peers: &BTreeMap<String, Vec<String>>,
    all_preferred_versions: &PreferredVersions,
    workspace_root_deps: &[WorkspaceRootDep],
    locked_peer_versions: &HashMap<String, HashSet<String>>,
) -> BTreeMap<String, String> {
    let mut optional_dependencies = BTreeMap::new();
    for (peer_name, ranges) in all_missing_optional_peers {
        let Some(selectors) = all_preferred_versions.get(peer_name) else { continue };
        // The workspace root's own specifier bounds the candidates the
        // same way it short-circuits `hoist_peers` above. Maximizing over
        // every version in the graph instead lets one importer's newer
        // resolution be hoisted into a sibling that declares nothing,
        // adding a second instance of a package the root already pins. A
        // scheme specifier bounds them through the version body
        // `get_peer_version_range` extracts; one with no version body
        // yields `*` and leaves them unbounded.
        let root_range = find_workspace_root_dep(workspace_root_deps, peer_name)
            .and_then(|dep| dep.normalized_bare_specifier.as_deref())
            .and_then(|spec| get_peer_version_range(spec).parse::<Range>().ok());
        // An unparsable range is satisfied by nothing, so bailing on the
        // peer matches failing the check per candidate.
        let Ok(parsed_ranges) =
            ranges.iter().map(|range| range.parse::<Range>()).collect::<Result<Vec<_>, _>>()
        else {
            continue;
        };
        // A disjoint specifier would bound the candidates down to none, and
        // the importer would then fall back to the root's own out-of-range
        // version.
        let root_range =
            root_range.filter(|root| parsed_ranges.iter().all(|parsed| root.allows_any(parsed)));
        let mut max_satisfying_version: Option<Version> = None;
        for (version_str, entry) in selectors {
            if locked_peer_versions
                .get(peer_name)
                .is_some_and(|versions| !versions.contains(version_str))
            {
                continue;
            }
            let selector_type = match entry {
                VersionSelectorEntry::Plain(selector_type) => *selector_type,
                VersionSelectorEntry::Weighted(weighted) => weighted.selector_type,
            };
            if selector_type != VersionSelectorType::Version {
                continue;
            }
            let Ok(version) = version_str.parse::<Version>() else { continue };
            if root_range.as_ref().is_some_and(|range| !range.satisfies(&version)) {
                continue;
            }
            // Strict, unlike the required-peer picker above: an optional
            // peer nobody declared is installed only to deduplicate, so a
            // prerelease its range rejects is not worth splitting a
            // package family over.
            if !parsed_ranges.iter().all(|parsed| parsed.satisfies(&version)) {
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

/// The root dependency that provides `peer_name`: an alias match wins
/// over a package-name match (an `npm:` alias can install the same
/// package under a different slot), and among package-name matches the
/// lexicographically first alias wins so the pick is stable. Only a
/// dependency that has a normalized specifier is a candidate — the
/// callers have nothing to install or bound the peer with otherwise.
fn find_workspace_root_dep<'a>(
    workspace_root_deps: &'a [WorkspaceRootDep],
    peer_name: &str,
) -> Option<&'a WorkspaceRootDep> {
    let candidates =
        || workspace_root_deps.iter().filter(|dep| dep.normalized_bare_specifier.is_some());
    candidates().find(|root_dep| root_dep.alias == peer_name).or_else(|| {
        candidates()
            .filter(|root_dep| root_dep.pkg_name == peer_name)
            .min_by(|a, b| a.alias.cmp(&b.alias))
    })
}

/// Highest version from `versions` that satisfies `range` under npm's
/// `includePrerelease` semantics. Returns `None` if no candidate
/// satisfies.
fn max_satisfying<'a>(versions: &'a [&'a str], range: &str) -> Option<&'a str> {
    let parsed_range = IncludePrereleaseRange::parse(range);
    let mut best: Option<(&str, Version)> = None;
    for spec in versions {
        let Ok(parsed_version) = spec.parse::<Version>() else { continue };
        if !parsed_range.satisfies(&parsed_version) {
            continue;
        }
        if best.as_ref().is_none_or(|(_, cur)| parsed_version > *cur) {
            best = Some((*spec, parsed_version));
        }
    }
    best.map(|(spec, _)| spec)
}

/// Highest version overall from `versions` (the `*` range). Returns
/// `None` when no candidate parses as a valid semver.
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
