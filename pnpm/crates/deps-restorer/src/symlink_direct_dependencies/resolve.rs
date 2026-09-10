use crate::{SkippedSnapshots, VirtualStoreLayout};
use pnpm_lockfile::{
    ImporterDepVersion, PkgName, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencySpec,
};
use pnpm_package_manifest::DependencyGroup;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

/// The wire `version` of a dep whose metadata row carries none.
pub(super) fn fallback_version(version: &ImporterDepVersion) -> String {
    match version {
        ImporterDepVersion::Regular(ver) => ver.version().to_string(),
        ImporterDepVersion::Alias(alias) => alias.suffix.version().to_string(),
        ImporterDepVersion::Link(target) => format!("link:{target}"),
        ImporterDepVersion::File(target) => format!("file:{target}"),
    }
}
/// One direct-dep entry plus its resolved on-disk target. The
/// target is computed eagerly so dedupe can compare it against the
/// root importer's targets and so the parallel symlink loop doesn't
/// recompute it.
pub(super) struct ResolvedEntry<'a> {
    pub(super) name: &'a PkgName,
    pub(super) spec: &'a ResolvedDependencySpec,
    pub(super) group: DependencyGroup,
    pub(super) name_str: String,
    pub(super) target: PathBuf,
}
/// Walk an importer snapshot's dependency groups and emit one
/// [`ResolvedEntry`] per direct dep, applying the same first-wins /
/// skipped / link-only filters that [`link_one_importer`](crate::symlink_direct_dependencies::link_one_importer) (private to
/// this module) uses to drive the symlink + bin-link pass.
///
/// Iterate per group so each emit can label the dependency with its
/// [`DependencyType`](pnpm_reporter::DependencyType). pnpm's reporter renders the diff with that
/// hint, so dropping it would silently misclassify devDependencies
/// as prod. [`ProjectSnapshot::dependencies_by_groups`] flattens the
/// groups together, which is convenient for the symlink loop but
/// loses the per-group identity we need for the emit.
///
/// Peers are filtered upfront: pnpm doesn't emit `pnpm:root` for
/// peer dependencies (they're materialised through their host
/// package, not directly under `node_modules/`), and
/// [`ProjectSnapshot::get_map_by_group`] also returns `None` for
/// `Peer` so this filter is belt-and-braces.
pub(super) fn collect_resolved_entries<'a>(
    layout: &VirtualStoreLayout,
    project_snapshot: &'a ProjectSnapshot,
    project_dir: &Path,
    dependency_groups: impl IntoIterator<Item = DependencyGroup>,
    skipped: &SkippedSnapshots,
    link_only: bool,
) -> Vec<ResolvedEntry<'a>> {
    let mut seen: HashSet<&PkgName> = HashSet::new();
    dependency_groups
        .into_iter()
        .filter(|group| !matches!(group, DependencyGroup::Peer))
        .flat_map(|group| {
            project_snapshot
                .get_map_by_group(group)
                .into_iter()
                .flatten()
                .map(move |(name, spec)| (name, spec, group))
        })
        .filter(|(name, _, _)| seen.insert(*name))
        // Drop direct deps whose resolved snapshot landed in the
        // skipped set. Without this filter, the symlink would
        // either dangle (no virtual-store slot was created) or —
        // worse — point at a half-installed slot from a prior
        // install. `link:` deps
        // never participate in the virtual store, so they are
        // exempt from the skipped check (the resolved snapshot key
        // wouldn't exist in the set anyway).
        .filter(|(name, spec, _)| match spec.version.resolved_key(name) {
            Some(resolved) => !skipped.contains(&resolved),
            // `link:` deps have no virtual-store slot and so
            // cannot be in `skipped` — keep them.
            None => true,
        })
        // Hoisted-mode filter: `link_only` keeps only `link:`
        // entries (workspace siblings) and drops every regular
        // dep. The hoisted linker (slice 5) already materialized
        // those regular deps as real `<importer>/node_modules/<alias>/`
        // directories; re-symlinking them here would either no-op
        // or replace the real dir with a slot symlink that points
        // at a slot that doesn't exist under hoisted.
        .filter(
            |(_, spec, _)| {
                if link_only { matches!(spec.version, ImporterDepVersion::Link(_)) } else { true }
            },
        )
        .map(|(name, spec, group)| {
            let name_str = name.to_string();
            let target = resolve_target_path(layout, project_dir, name, spec, &name_str);
            ResolvedEntry { name, spec, group, name_str, target }
        })
        .collect()
}
/// Map a `(name, spec)` to the on-disk path a direct-dep symlink
/// should point at. Pulled out of the rayon loop so [`collect_resolved_targets`]
/// can reuse the same computation when building the dedupe map.
pub(super) fn resolve_target_path(
    layout: &VirtualStoreLayout,
    project_dir: &Path,
    name: &PkgName,
    spec: &ResolvedDependencySpec,
    name_str: &str,
) -> PathBuf {
    match &spec.version {
        ImporterDepVersion::Regular(ver_peer) => {
            // Route the slot-directory lookup through the
            // install-scoped [`VirtualStoreLayout`] so the path
            // works under both legacy
            // (`<virtual_store_dir>/<flat-name>`) and GVS
            // (`<global_virtual_store_dir>/<scope>/<name>/<version>/<hash>`)
            // layouts. The layout's GVS-suffix map is keyed by the
            // full snapshot key (with peer suffix), so construct
            // that from the importer's resolved version-with-peer
            // rather than from `name`+`version` separately.
            let dep_key = PkgNameVerPeer::new(PkgName::clone(name), ver_peer.clone());
            layout.slot_dir(&dep_key).join("node_modules").join(name_str)
        }
        ImporterDepVersion::Alias(alias) => {
            // For an alias, the snapshot key carries the resolved
            // package's real name + version-with-peer, and the inner
            // `node_modules/<real-name>` directory is named after that
            // real name (not the importer-map key). The on-disk
            // symlink at `<modules_dir>/<importer-key>` still uses
            // `name_str` as the link name.
            layout.slot_dir(alias).join("node_modules").join(alias.name.to_string())
        }
        ImporterDepVersion::Link(target) => {
            // `link:<path>` values are relative to the importer's
            // `rootDir` (or absolute). Resolve them here so the
            // on-disk symlink points at the right sibling project.
            // pacquet's lockfile snapshot already carries the
            // raw `link:` payload, so the resolution lives at the
            // install layer.
            //
            // Run the joined result through `lexical_normalize` so
            // the dedupe pass treats `<workspace>/packages/a` and
            // `<workspace>/packages/foo/../a` as the same target.
            // The dedupe compares stored symlink targets via
            // `path.relative(a, b) === ''`, which on absolute paths
            // reduces to lexical equality *after* both arguments pass
            // through `path.resolve` (Node normalises by default).
            // `Path::join` does not, so we have to do it explicitly
            // here.
            let candidate = Path::new(target);
            let joined = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                project_dir.join(candidate)
            };
            pnpm_fs::lexical_normalize(&joined)
        }
        ImporterDepVersion::File(_) => {
            // Injected workspace dep that didn't dedupe back to
            // `link:` — the importer entry references a virtual-store
            // slot keyed by `(importer_key, file:<payload>)`. Route
            // through `resolved_key` so the layout's GVS-suffix map
            // sees the same key the snapshot writer used.
            let dep_key =
                spec.version.resolved_key(name).expect("File arm always produces a resolved_key");
            layout.slot_dir(&dep_key).join("node_modules").join(name_str)
        }
    }
}
/// Build the `<alias → resolved-target>` map a dedupe pass needs
/// for a single importer (always the root). Applies the same
/// per-importer filters as [`collect_resolved_entries`] so the map
/// only contains aliases that would have been symlinked — equivalent
/// to reading the root's `node_modules/` after its direct deps are
/// linked, which by then contains exactly the entries that survived
/// the skipped / link-only filters.
pub(super) fn collect_resolved_targets(
    layout: &VirtualStoreLayout,
    project_snapshot: &ProjectSnapshot,
    project_dir: &Path,
    dependency_groups: impl IntoIterator<Item = DependencyGroup>,
    skipped: &SkippedSnapshots,
    link_only: bool,
) -> BTreeMap<String, PathBuf> {
    collect_resolved_entries(
        layout,
        project_snapshot,
        project_dir,
        dependency_groups,
        skipped,
        link_only,
    )
    .into_iter()
    .map(|entry| (entry.name_str, entry.target))
    .collect()
}
