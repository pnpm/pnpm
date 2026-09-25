use super::super::{
    DependencyGroup, ResolveImporterOptions, SortedImporters, WorkspaceImporter, WorkspaceTreeCtx,
};
use crate::resolve_dependency_tree::importer_direct_wanted_specs;
use node_semver::{Range, Version};
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_resolving_resolver_base::{
    EXISTING_VERSION_SELECTOR_WEIGHT, VersionSelectorEntry, VersionSelectorType,
    VersionSelectorWithWeight,
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

/// Re-resolve the auto-installed peers [`find_stale_peer_pins`] finds
/// instead of reusing their lockfile pins. Must run before the importers
/// share `workspace`.
pub(super) fn release_stale_peer_pins(
    workspace: &mut Arc<WorkspaceTreeCtx>,
    sorted: &mut SortedImporters<'_, '_>,
) {
    let Some(lockfile) = workspace.wanted_lockfile().cloned() else {
        return;
    };
    let stale_peer_pins = find_stale_peer_pins(&sorted.importers, &sorted.opts, &lockfile);
    for (importer, opts) in sorted.importers.iter().zip(sorted.opts.iter_mut()) {
        if let Some(stale) = stale_peer_pins.get(&importer.id) {
            unpin_stale_peers(opts, stale);
        }
    }
    let stale_aliases = stale_peer_pins
        .into_iter()
        .map(|(importer_id, stale)| (importer_id, stale.into_keys().collect()))
        .collect();
    Arc::get_mut(workspace)
        .expect("the workspace ctx is not shared before the importers initialize")
        .set_stale_peer_pins(stale_aliases);
}

/// Per importer and by alias, the locked versions of the auto-installed
/// peers (peer dependencies it does not also declare as a dependency)
/// that no workspace
/// project's specifier for that package accepts any more, although one
/// of them still overlaps the peer range. Such a peer has to be
/// re-resolved, or it stays on its first resolution while the projects
/// that provide it move on (pnpm/pnpm#11800).
fn find_stale_peer_pins(
    importers: &[&WorkspaceImporter<'_>],
    opts: &[ResolveImporterOptions],
    lockfile: &Lockfile,
) -> HashMap<String, HashMap<String, Version>> {
    if !importers.iter().any(|importer| declares_peers(importer)) {
        return HashMap::default();
    }
    let direct_ranges = collect_direct_ranges(importers, opts);
    let mut stale = HashMap::<String, HashMap<String, Version>>::default();
    for importer in importers.iter().filter(|importer| declares_peers(importer)) {
        let peers = importer_stale_peer_pins(importer, lockfile, &direct_ranges);
        if !peers.is_empty() {
            stale.insert(importer.id.clone(), peers);
        }
    }
    stale
}

/// The distinct ranges the importers declare for each package in their
/// `dependencies`, `devDependencies` and `optionalDependencies`, with
/// `catalog:` specifiers replaced by the catalog entry.
fn collect_direct_ranges(
    importers: &[&WorkspaceImporter<'_>],
    opts: &[ResolveImporterOptions],
) -> HashMap<String, HashSet<Range>> {
    let mut direct_ranges = HashMap::<String, HashSet<Range>>::default();
    for (importer, importer_opts) in importers.iter().zip(opts) {
        let Ok(wanted) = importer_direct_wanted_specs(
            importer.manifest,
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
            false,
            &importer_opts.resolution.catalogs,
            importer_opts.resolution.catalogs_dir.as_deref(),
        ) else {
            continue;
        };
        for (alias, spec, ..) in wanted {
            if let Ok(range) = spec.parse::<Range>() {
                direct_ranges
                    .entry(alias)
                    .or_default()
                    .insert(range);
            }
        }
    }
    direct_ranges
}

fn declares_peers(importer: &WorkspaceImporter<'_>) -> bool {
    importer.manifest
        .dependencies([DependencyGroup::Peer])
        .next()
        .is_some()
}

fn importer_stale_peer_pins(
    importer: &WorkspaceImporter<'_>,
    lockfile: &Lockfile,
    direct_ranges: &HashMap<String, HashSet<Range>>,
) -> HashMap<String, Version> {
    let manifest = importer.manifest;
    let declared = manifest
        .dependencies([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional])
        .map(|(name, _)| name)
        .collect::<HashSet<_>>();
    manifest
        .dependencies([DependencyGroup::Peer])
        .filter(|(alias, _)| !declared.contains(alias))
        .filter_map(|(alias, peer_spec)| {
            let ranges = direct_ranges.get(alias)?;
            let peer_range = peer_spec.parse::<Range>().ok()?;
            let pinned = locked_importer_version(lockfile, &importer.id, alias)?;
            let mut overlapping = ranges
                .iter()
                .filter(|range| range.allows_any(&peer_range))
                .peekable();
            overlapping.peek()?;
            (!overlapping.any(|range| range.satisfies(&pinned))).then(|| {
                (alias.to_string(), pinned)
            })
        })
        .collect()
}

fn locked_importer_version(lockfile: &Lockfile, importer_id: &str, alias: &str) -> Option<Version> {
    let name = alias.parse::<PkgName>().ok()?;
    let importer = lockfile.importers.get(importer_id)?;
    [&importer.dependencies, &importer.optional_dependencies, &importer.dev_dependencies]
        .into_iter()
        .find_map(|deps| deps.as_ref()?.get(&name))?
        .version
        .ver_peer()?
        .version_semver()
        .cloned()
}

/// Remove the lockfile's weight from the stale locked versions in the
/// importer's preferred versions, so the peers are picked the way a fresh
/// install picks them rather than back onto the stale versions.
fn unpin_stale_peers(opts: &mut ResolveImporterOptions, stale: &HashMap<String, Version>) {
    let preferred_versions = Arc::make_mut(&mut opts.base_opts.version.preferred_versions);
    for (name, pinned) in stale {
        let Some(selectors) = preferred_versions.get_mut(name) else { continue };
        let pinned = pinned.to_string();
        let Some(VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight,
        })) = selectors.get_mut(&pinned)
        else {
            continue;
        };
        if *weight < EXISTING_VERSION_SELECTOR_WEIGHT {
            continue;
        }
        *weight -= EXISTING_VERSION_SELECTOR_WEIGHT;
        if *weight == 0 {
            selectors.remove(&pinned);
        }
    }
}
