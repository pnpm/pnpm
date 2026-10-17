//! Picks the version that provides a missing peer when it is hoisted to
//! the importer. The orchestrator (`resolve_importer`) calls it inside
//! its hoist loop for a "what to add to the importer's direct deps" map.
//!
//! Both pickers go through [`highest_provider_version`]. They differ only
//! in their tiers and range check:
//!
//! | Peer     | Tiers                       | Range check                    |
//! | -------- | --------------------------- | ------------------------------ |
//! | required | [`ProviderTiers::required`] | `includePrerelease`, one range |
//! | optional | [`ProviderTiers::optional`] | strict, every recorded range   |

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, path::Path};

use node_semver::{Range, Version};
use pnpm_resolving_resolver_base::{
    PreferredVersions, VersionSelectorEntry, VersionSelectorType, VersionSelectors,
    get_peer_version_range,
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
    /// The versions of each peer that this run has resolved. A satisfying
    /// one wins over a version that only the wanted lockfile holds.
    pub resolved_versions: &'a HashMap<String, HashSet<String>>,
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
        if let Some(specifier) = hoisted_specifier(opts, peer_name, &info.range) {
            dependencies.insert(peer_name.clone(), specifier);
        }
    }
    dependencies
}

/// The specifier one missing required peer hoists to, or `None` when it must
/// stay missing.
fn hoisted_specifier(opts: &HoistPeersOptions<'_>, peer_name: &str, range: &str) -> Option<String> {
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
        return (overridden != "-").then_some(overridden);
    }
    if let Some(spec) = root_bare_specifier {
        return Some(spec.clone());
    }
    let Some(selectors) = opts.all_preferred_versions.get(peer_name) else {
        return opts.auto_install_peers.then(|| range.to_string());
    };
    // A `workspace:` range resolves to the workspace project. A preferred
    // version may belong to a registry package of the same name, so the
    // peer is not deduped onto one.
    if range.starts_with("workspace:") {
        return opts.auto_install_peers.then(|| range.to_string());
    }
    preferred_version_specifier(opts, selectors, opts.resolved_versions.get(peer_name), range)
}

/// Dedupe onto a preferred version only when it actually satisfies the wanted
/// peer range. Picking the highest preferred version regardless of the range
/// lets a version resolved for one importer be auto-installed as another
/// importer's peer even though nothing in that importer's closure accepts it,
/// silently producing a peer graph that mixes incompatible majors. Scheme
/// specifiers (named-registry, `npm:` aliases) contribute a
/// comparable range through `get_peer_version_range`, so they get range-aware
/// selection too; specs with no version body (`catalog:`, dist-tags) yield a
/// non-semver value and keep the dedupe-to-highest behavior. The raw scheme is
/// preserved so the fallback still selects the package to install.
fn preferred_version_specifier(
    opts: &HoistPeersOptions<'_>,
    selectors: &VersionSelectors,
    resolved_versions: Option<&HashSet<String>>,
    range: &str,
) -> Option<String> {
    let non_versions = non_version_selectors(selectors);
    let range_for_match = get_peer_version_range(range);
    let is_semver_range = range_for_match.parse::<Range>().is_ok();
    if is_semver_range {
        let parsed_range = IncludePrereleaseRange::parse(&range_for_match);
        let satisfying = highest_provider_version(
            selectors,
            &ProviderTiers::required(resolved_versions),
            |_, version| parsed_range.satisfies(version),
        );
        if let Some((satisfying, _)) = satisfying {
            let mut parts: Vec<&str> = vec![satisfying];
            parts.extend(non_versions.iter().copied());
            return Some(parts.join(" || "));
        }
        if has_version_selector(selectors) {
            // Preferred versions exist but none satisfies the wanted
            // range. Use the range directly so it resolves from the
            // registry rather than installing a version the peer
            // explicitly rejects. Without auto-install-peers, hoist
            // nothing and leave the peer missing.
            return opts.auto_install_peers.then(|| range.to_string());
        }
    }
    let mut parts: Vec<&str> = Vec::new();
    if let Some((highest, _)) =
        highest_provider_version(selectors, &ProviderTiers::default(), |_, _| true)
    {
        parts.push(highest);
    }
    parts.extend(non_versions.iter().copied());
    (!parts.is_empty()).then(|| parts.join(" || "))
}

fn selector_type(entry: &VersionSelectorEntry) -> VersionSelectorType {
    match entry {
        VersionSelectorEntry::Plain(selector_type) => *selector_type,
        VersionSelectorEntry::Weighted(weighted) => weighted.selector_type,
    }
}

fn non_version_selectors(selectors: &VersionSelectors) -> Vec<&str> {
    selectors
        .iter()
        .filter(|(_, entry)| selector_type(entry) != VersionSelectorType::Version)
        .map(|(spec, _)| spec.as_str())
        .collect()
}

fn has_version_selector(selectors: &VersionSelectors) -> bool {
    selectors
        .values()
        .any(|entry| selector_type(entry) == VersionSelectorType::Version)
}

/// The order in which a peer's versions are tried as its provider. The
/// first tier holding a qualifying version wins. When none does, every
/// version competes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ProviderTiers(Vec<HashSet<String>>);

impl ProviderTiers {
    /// The versions this run `resolved`. A version only the wanted
    /// lockfile holds is a weight whose provider has moved on, so it must
    /// not outrank one this run resolved.
    fn required(resolved: Option<&HashSet<String>>) -> Self {
        Self(resolved.cloned().into_iter().collect())
    }

    /// The `locked` versions this run `resolved`, then every version it
    /// resolved, then the `locked` ones. A locked version the run no
    /// longer resolves survives only as a lockfile weight, since whatever
    /// provided it has moved on, so it must not outrank what the graph
    /// now holds.
    pub(crate) fn optional(locked: Option<&HashSet<String>>, resolved: HashSet<String>) -> Self {
        let locked = locked.cloned().unwrap_or_default();
        let reached: HashSet<String> = locked
            .intersection(&resolved)
            .cloned()
            .collect();
        Self(
            [reached, resolved, locked]
                .into_iter()
                .filter(|tier| !tier.is_empty())
                .collect(),
        )
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The highest plain version among `selectors` that `qualifies`, from the
/// first of `tiers` that holds one. On equal versions the first selector
/// wins.
///
/// A tier without a qualifying version is stale and does not veto the
/// pick. Its provider left the graph, or it fell outside the ranges.
/// Leaving an optional peer bare drops it from the direct dependency key,
/// and the run after that re-attaches it from these very candidates.
fn highest_provider_version<'a>(
    selectors: &'a VersionSelectors,
    tiers: &ProviderTiers,
    qualifies: impl Fn(&str, &Version) -> bool,
) -> Option<(&'a str, Version)> {
    let highest_in = |tier: Option<&HashSet<String>>| {
        let mut best: Option<(&'a str, Version)> = None;
        for (spec, version) in plain_versions_in(selectors, tier) {
            if qualifies(spec, &version)
                && best
                    .as_ref()
                    .is_none_or(|(_, cur)| version > *cur)
            {
                best = Some((spec, version));
            }
        }
        best
    };
    tiers.0
        .iter()
        .find_map(|tier| highest_in(Some(tier)))
        .or_else(|| highest_in(None))
}

/// The plain versions among `selectors`, limited to `tier` when one is given.
fn plain_versions_in<'a>(
    selectors: &'a VersionSelectors,
    tier: Option<&HashSet<String>>,
) -> impl Iterator<Item = (&'a str, Version)> {
    selectors
        .iter()
        .filter(move |(spec, entry)| {
            selector_type(entry) == VersionSelectorType::Version
                && tier.is_none_or(|tier| tier.contains(*spec))
        })
        .filter_map(|(spec, _)| Some((spec.as_str(), spec.parse::<Version>().ok()?)))
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
    get_hoistable_optional_peers_with_preferred_versions(
        all_missing_optional_peers,
        all_preferred_versions,
        workspace_root_deps,
        &HashMap::default(),
        &|_, _| true,
    )
}

static NO_TIERS: ProviderTiers = ProviderTiers(Vec::new());

pub(crate) fn get_hoistable_optional_peers_with_preferred_versions(
    all_missing_optional_peers: &BTreeMap<String, Vec<String>>,
    all_preferred_versions: &PreferredVersions,
    workspace_root_deps: &[WorkspaceRootDep],
    preferred_peer_versions: &HashMap<String, ProviderTiers>,
    accepts_candidate: &dyn Fn(&str, &str) -> bool,
) -> BTreeMap<String, String> {
    let mut optional_dependencies = BTreeMap::new();
    for (peer_name, ranges) in all_missing_optional_peers {
        let Some(selectors) = all_preferred_versions.get(peer_name) else { continue };
        let version = max_hoistable_optional_version(
            selectors,
            ranges,
            find_workspace_root_dep(workspace_root_deps, peer_name),
            preferred_peer_versions.get(peer_name).unwrap_or(&NO_TIERS),
            &|version| accepts_candidate(peer_name, version),
        );
        if let Some(version) = version {
            optional_dependencies.insert(peer_name.clone(), version.to_string());
        }
    }
    optional_dependencies
}

/// The highest preferred version satisfying every range recorded for one
/// missing optional peer, or `None` when no candidate qualifies.
fn max_hoistable_optional_version(
    selectors: &VersionSelectors,
    ranges: &[String],
    root_dep: Option<&WorkspaceRootDep>,
    tiers: &ProviderTiers,
    accepts_candidate: &dyn Fn(&str) -> bool,
) -> Option<Version> {
    // An unparsable range is satisfied by nothing, so bailing on the
    // peer matches failing the check per candidate.
    let parsed_ranges: Vec<Range> = ranges
        .iter()
        .map(|range| range.parse::<Range>())
        .collect::<Result<_, _>>()
        .ok()?;
    // The workspace root's own specifier bounds the candidates the
    // same way it short-circuits `hoist_peers` above. Maximizing over
    // every version in the graph instead lets one importer's newer
    // resolution be hoisted into a sibling that declares nothing,
    // adding a second instance of a package the root already pins. A
    // scheme specifier bounds them through the version body
    // `get_peer_version_range` extracts; one with no version body
    // yields `*` and leaves them unbounded. A disjoint specifier would
    // bound the candidates down to none, and the importer would then
    // fall back to the root's own out-of-range version, so it is dropped.
    let root_range = root_dep
        .and_then(|dep| dep.normalized_bare_specifier.as_deref())
        .and_then(|spec| get_peer_version_range(spec).parse::<Range>().ok())
        .filter(|root| {
            parsed_ranges
                .iter()
                .all(|parsed| root.allows_any(parsed))
        });
    // Strict, unlike the required-peer picker above: an optional
    // peer nobody declared is installed only to deduplicate, so a
    // prerelease its range rejects is not worth splitting a
    // package family over.
    let satisfies_ranges = |version: &Version| {
        root_range
            .as_ref()
            .is_none_or(|range| range.satisfies(version))
            && parsed_ranges
                .iter()
                .all(|parsed| parsed.satisfies(version))
    };
    highest_provider_version(selectors, tiers, |spec, version| {
        satisfies_ranges(version) && accepts_candidate(spec)
    })
    .map(|(_, version)| version)
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
    candidates()
        .find(|root_dep| root_dep.alias == peer_name)
        .or_else(|| {
            candidates()
                .filter(|root_dep| root_dep.pkg_name == peer_name)
                .min_by(|a, b| a.alias.cmp(&b.alias))
        })
}

/// The specifier an optional peer picked at `version` is installed with: the
/// `workspace:` specifier of the root dependency that provides `name` when it
/// resolved to `version`, otherwise `version`.
pub(crate) fn optional_peer_specifier(
    name: &str,
    version: String,
    workspace_root_deps: &[WorkspaceRootDep],
    workspace_root_dep_versions: &HashMap<String, String>,
) -> String {
    find_workspace_root_dep(workspace_root_deps, name)
        .filter(|dep| workspace_root_dep_versions.get(&dep.alias) == Some(&version))
        .and_then(|dep| dep.normalized_bare_specifier.as_ref())
        .filter(|specifier| specifier.starts_with("workspace:"))
        .cloned()
        .unwrap_or(version)
}

#[cfg(test)]
mod tests;
