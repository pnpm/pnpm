//! Port of pnpm's
//! [`resolveRootDependencies`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L327-L437)
//! — the multi-pass loop that hoists missing peers into the importer's
//! direct deps until no required peer is missing and no optional peer
//! is satisfiable from the in-flight preferred-versions map.
//!
//! Two nested fixed-point loops:
//!
//! 1. **Inner / required pass.** Run [`fn@crate::resolve_peers`] over the
//!    growing tree, collect peers that are required (not optional) and
//!    not already direct deps, pick a specifier per peer via
//!    [`fn@crate::hoist_peers`], and extend the tree with those picks.
//!    Repeat until the picker proposes nothing new.
//! 2. **Outer / optional pass.** Aggregate the optional missing peers
//!    seen across the inner-loop iterations, ask
//!    [`fn@crate::get_hoistable_optional_peers`] which of them have a
//!    preferred version already in scope, and extend the tree with
//!    those. Re-enter the inner loop if any landed.
//!
//! Single-importer slice; multi-importer support waits on pacquet's
//! workspace work.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_types::Catalogs;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_patching::PatchGroupRecord;
use pacquet_resolving_resolver_base::{
    PreferredVersions, ResolveOptions, Resolver, VersionSelectorEntry, VersionSelectorType,
};

use crate::{
    DirectDep,
    dependencies_graph::MissingPeer,
    hoist_peers::{
        HoistPeersOptions, MissingPeerInfo, WorkspaceRootDep, get_hoistable_optional_peers,
        hoist_peers,
    },
    resolve_dependency_tree::{
        ResolveDependencyTreeError, TreeCtx, extend_tree, importer_optional_dependency_names,
        resolve_catalog_specifiers,
    },
    resolve_peers::{ResolvePeersOptions, ResolvePeersResult, resolve_peers},
    resolved_tree::ResolvedTree,
};

/// Options threaded into [`fn@resolve_importer`].
#[derive(Debug)]
pub struct ResolveImporterOptions {
    /// When true, missing required peers get installed at the importer
    /// even if no preferred version is in scope (the picker uses the
    /// peer's declared range as the specifier).
    pub auto_install_peers: bool,

    /// When true, conflicting peer ranges from multiple consumers are
    /// merged with `||` instead of being dropped on intersection
    /// failure. Mirrors pnpm's `autoInstallPeersFromHighestMatch`.
    pub auto_install_peers_from_highest_match: bool,

    /// When true, the importer's direct deps are used as
    /// `workspace_root_deps` for the hoist picker so a peer matching a
    /// direct dep's alias / name short-circuits straight to that
    /// dep's specifier. Single-importer pacquet treats the importer
    /// as the root.
    pub resolve_peers_from_workspace_root: bool,

    /// Seed for the preferred-versions tie-break table. The
    /// orchestrator extends this in place as packages are walked —
    /// each newly-resolved `name@version` lands as a plain
    /// [`VersionSelectorType::Version`] entry so [`hoist_peers`] and
    /// [`get_hoistable_optional_peers`] can reuse it. Pass the result
    /// of `get_preferred_versions_from_lockfile_and_manifests` from
    /// the `lockfile-preferred-versions` crate, or an empty map when
    /// no lockfile + manifest seeding is available.
    pub all_preferred_versions: PreferredVersions,

    /// Configured `patchedDependencies`, grouped by package name. The
    /// tree walker appends `(patch_hash=<hash>)` to each matched
    /// package's `pkgIdWithPatchHash` and records the matched key on
    /// [`crate::ResolvedTree::applied_patches`]. `None` when no
    /// patches are configured for this install.
    pub patched_dependencies: Option<Arc<PatchGroupRecord>>,

    pub base_opts: ResolveOptions,

    /// Catalogs parsed from `pnpm-workspace.yaml`. Applied only to the
    /// importer's direct dependencies; transitive `catalog:` entries
    /// are not resolved through the catalog, matching upstream's
    /// [importer-only catalog scope](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/installing/deps-resolver/src/resolveDependencies.ts#L592-L600).
    pub catalogs: Catalogs,
}

/// Result of [`fn@resolve_importer`] — the fully-walked tree plus the
/// peer-resolution output the install layer consumes.
#[derive(Debug)]
pub struct ResolveImporterResult {
    pub resolved_tree: ResolvedTree,
    pub peers_result: ResolvePeersResult,
}

/// Error envelope for [`fn@resolve_importer`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveImporterError {
    #[display("{_0}")]
    Resolve(#[error(source)] ResolveDependencyTreeError),
}

impl From<ResolveDependencyTreeError> for ResolveImporterError {
    fn from(err: ResolveDependencyTreeError) -> Self {
        ResolveImporterError::Resolve(err)
    }
}

/// Resolve an importer's full dependency graph with auto-install-peers
/// hoisting. See the module-level doc for the algorithm.
pub async fn resolve_importer<DependencyGroupList>(
    resolver: Arc<dyn Resolver>,
    manifest: &PackageManifest,
    dependency_groups: DependencyGroupList,
    opts: ResolveImporterOptions,
) -> Result<ResolveImporterResult, ResolveImporterError>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    let ResolveImporterOptions {
        auto_install_peers,
        auto_install_peers_from_highest_match,
        resolve_peers_from_workspace_root,
        mut all_preferred_versions,
        patched_dependencies,
        base_opts,
        catalogs,
    } = opts;

    let ctx = TreeCtx::new(resolver, base_opts).with_patched_dependencies(patched_dependencies);

    // Mirrors upstream's
    // [`getAllDependenciesFromManifest({ autoInstallPeers })`](https://github.com/pnpm/pnpm/blob/097983fbca/pkg-manifest/utils/src/getAllDependenciesFromManifest.ts):
    // when auto-install-peers is on, the importer's own
    // `peerDependencies` are walked as regular direct deps too, so the
    // version the importer pinned wins over whatever a deeply-nested
    // consumer asks for via the hoist loop.
    let mut groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();
    if auto_install_peers && !groups.contains(&DependencyGroup::Peer) {
        groups.push(DependencyGroup::Peer);
    }
    // `optionalDependencies` wins over the other groups when an alias
    // appears in more than one — same precedence as upstream's
    // [`getWantedDependenciesFromGivenSet`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/getWantedDependencies.ts#L57-L72).
    // Pre-compute the optional name set so the walker can tag each
    // direct dep with the right `wanted.optional` flag for the
    // `ResolvedPackage.optional` propagation.
    let optional_names = importer_optional_dependency_names(manifest);
    let initial_wanted: Vec<(String, String, bool)> = manifest
        .dependencies(groups)
        .map(|(name, range)| {
            let optional = optional_names.contains(name);
            (name.to_string(), range.to_string(), optional)
        })
        .collect();
    let initial_wanted = resolve_catalog_specifiers(initial_wanted, &catalogs)?;
    let mut direct = extend_tree(&ctx, initial_wanted).await?;
    update_preferred_versions_with_ctx(&ctx, &mut all_preferred_versions);

    let mut parent_pkg_aliases: HashSet<String> =
        direct.iter().map(|dep| dep.alias.clone()).collect();
    let mut all_missing_optional_peers: BTreeMap<String, Vec<String>> = BTreeMap::new();

    loop {
        loop {
            let snapshot = ctx.snapshot(direct.clone());
            let peers_result = resolve_peers(&snapshot, ResolvePeersOptions::default());

            let (missing_required, fresh_optional) = partition_missing_peers(
                &peers_result.peer_dependency_issues.missing,
                &parent_pkg_aliases,
                auto_install_peers_from_highest_match,
            );
            for (name, ranges) in fresh_optional {
                let bucket = all_missing_optional_peers.entry(name).or_default();
                for range in ranges {
                    if !bucket.iter().any(|existing| existing == &range) {
                        bucket.push(range);
                    }
                }
            }

            if missing_required.is_empty() {
                break;
            }

            let workspace_root_deps = if resolve_peers_from_workspace_root {
                build_workspace_root_deps(&direct, &snapshot)
            } else {
                Vec::new()
            };

            let missing_as_pairs: Vec<(String, MissingPeerInfo)> =
                missing_required.iter().map(|(n, info)| (n.clone(), info.clone())).collect();
            let hoisted = hoist_peers(
                HoistPeersOptions {
                    auto_install_peers,
                    all_preferred_versions: &all_preferred_versions,
                    workspace_root_deps: &workspace_root_deps,
                },
                &missing_as_pairs,
            );
            if hoisted.is_empty() {
                break;
            }

            for name in hoisted.keys() {
                parent_pkg_aliases.insert(name.clone());
            }

            // Hoisted required peers are installed at the importer
            // level as non-optional direct deps — they exist precisely
            // to satisfy a missing required peer, so flipping their
            // own `optional` flag to `true` would defeat the
            // auto-install. Mirrors upstream's `wantedDependency`
            // shape inside `hoistPeers`.
            let new_wanted: Vec<(String, String, bool)> =
                hoisted.into_iter().map(|(name, range)| (name, range, false)).collect();
            let new_direct = extend_tree(&ctx, new_wanted).await?;
            direct.extend(new_direct);
            update_preferred_versions_with_ctx(&ctx, &mut all_preferred_versions);
        }

        if all_missing_optional_peers.is_empty() {
            break;
        }
        let hoisted_optional =
            get_hoistable_optional_peers(&all_missing_optional_peers, &all_preferred_versions);
        if hoisted_optional.is_empty() {
            break;
        }
        for name in hoisted_optional.keys() {
            parent_pkg_aliases.insert(name.clone());
        }
        // Optional peers picked up via `getHoistableOptionalPeers` are
        // also installed at the importer level — the picker already
        // confirmed a preferred version is in scope. Treating them as
        // non-optional matches the required-peer arm above.
        let new_wanted: Vec<(String, String, bool)> =
            hoisted_optional.into_iter().map(|(name, range)| (name, range, false)).collect();
        let new_direct = extend_tree(&ctx, new_wanted).await?;
        direct.extend(new_direct);
        update_preferred_versions_with_ctx(&ctx, &mut all_preferred_versions);
        all_missing_optional_peers.clear();
    }

    let resolved_tree = ctx.into_resolved_tree(direct);
    let peers_result = resolve_peers(&resolved_tree, ResolvePeersOptions::default());
    Ok(ResolveImporterResult { resolved_tree, peers_result })
}

/// Split the missing-peer report into the inputs the inner and outer
/// loops consume.
///
/// A peer name is **required** for this iteration when at least one of
/// its consumers declared it non-optional and it isn't already in
/// `parent_pkg_aliases` (i.e. not already a direct dep that just
/// hadn't been added to the alias set yet). Its merged range follows
/// upstream's [`mergePkgsDeps`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L796-L818):
/// single-range cases pass through, multi-range cases intersect via a
/// stub (see [`merge_ranges`]) and fall through to `||`-join when
/// `auto_install_peers_from_highest_match` is set.
///
/// Peers whose consumers are *all* optional are returned as the second
/// component, keyed by peer name with the deduplicated range list the
/// outer loop's [`get_hoistable_optional_peers`] needs.
fn partition_missing_peers(
    missing: &std::collections::HashMap<String, Vec<MissingPeer>>,
    parent_pkg_aliases: &HashSet<String>,
    auto_install_peers_from_highest_match: bool,
) -> (BTreeMap<String, MissingPeerInfo>, BTreeMap<String, Vec<String>>) {
    let mut missing_required: BTreeMap<String, MissingPeerInfo> = BTreeMap::new();
    let mut missing_optional: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (peer_name, entries) in missing {
        if parent_pkg_aliases.contains(peer_name) {
            continue;
        }
        let required_ranges: Vec<&str> = entries
            .iter()
            .filter(|entry| !entry.optional)
            .map(|entry| entry.wanted_range.as_str())
            .collect();
        if required_ranges.is_empty() {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut ordered: Vec<String> = Vec::new();
            for entry in entries {
                if seen.insert(entry.wanted_range.clone()) {
                    ordered.push(entry.wanted_range.clone());
                }
            }
            if !ordered.is_empty() {
                missing_optional.insert(peer_name.clone(), ordered);
            }
            continue;
        }
        if let Some(range) = merge_ranges(&required_ranges, auto_install_peers_from_highest_match) {
            missing_required.insert(peer_name.clone(), MissingPeerInfo { range });
        }
    }
    (missing_required, missing_optional)
}

/// Combine multiple consumers' wanted ranges into a single specifier.
/// Single-range cases pass through unchanged. Multi-range cases that
/// reduce to one unique string also pass through. Anything else returns
/// `Some(joined)` when `auto_install_peers_from_highest_match` is set,
/// or `None` otherwise — same drop-on-conflict shape as upstream's
/// `mergePkgsDeps` when `safeIntersect` returns null.
///
/// Pacquet's `safe_intersect` stand-in is exact only for the
/// "single unique range" case; broader semver-range intersection
/// would need a port of the `semver-range-intersect` npm package.
/// In practice the multi-consumer non-identical case is rare enough
/// that conservative "no merge" matches upstream's `intersection ===
/// null` arm for the slice.
fn merge_ranges(ranges: &[&str], auto_install_peers_from_highest_match: bool) -> Option<String> {
    if ranges.len() == 1 {
        return Some(ranges[0].to_string());
    }
    let unique: BTreeSet<&&str> = ranges.iter().collect();
    if unique.len() == 1 {
        return Some(ranges[0].to_string());
    }
    if auto_install_peers_from_highest_match {
        return Some(ranges.join(" || "));
    }
    None
}

/// Build the [`WorkspaceRootDep`] slice the hoist picker sees. Reads
/// `direct` for the importer's slot names and `snapshot` for the
/// resolved package each slot points at — `normalized_bare_specifier`
/// passes through from the resolver verbatim.
fn build_workspace_root_deps(
    direct: &[DirectDep],
    snapshot: &ResolvedTree,
) -> Vec<WorkspaceRootDep> {
    let mut out = Vec::with_capacity(direct.len());
    for dep in direct {
        let Some(pkg) = snapshot.packages.get(&dep.id) else { continue };
        // `name_ver` is `None` for resolvers that learn the name from
        // the manifest only after the fetch (git / tarball / local).
        // Workspace-root short-circuit needs the resolution-time real
        // name, so skip those — they fall through to the
        // preferred-versions arm of `hoist_peers` instead.
        let Some(name_ver) = pkg.result.name_ver.as_ref() else { continue };
        out.push(WorkspaceRootDep {
            alias: dep.alias.clone(),
            pkg_name: name_ver.name.to_string(),
            normalized_bare_specifier: pkg.result.normalized_bare_specifier.clone(),
        });
    }
    out
}

/// Add every newly-resolved `name@version` from `ctx` to
/// `preferred` as a plain [`VersionSelectorType::Version`] entry,
/// mirroring upstream's per-resolve `allPreferredVersions[name][version]
/// = 'version'` assignment at
/// [`resolveDependencies.ts:1440`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1440).
/// Idempotent: only inserts when no entry exists for `(name, version)`.
fn update_preferred_versions_with_ctx(ctx: &TreeCtx, preferred: &mut PreferredVersions) {
    for (name, version) in ctx.resolved_versions() {
        let bucket = preferred.entry(name).or_default();
        bucket.entry(version).or_insert(VersionSelectorEntry::Plain(VersionSelectorType::Version));
    }
}

#[cfg(test)]
mod tests;
