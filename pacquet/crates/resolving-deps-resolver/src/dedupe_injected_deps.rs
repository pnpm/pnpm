//! Port of pnpm's
//! [`dedupeInjectedDeps`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/dedupeInjectedDeps.ts).
//!
//! Runs at the tail of the multi-importer [`fn@crate::resolve_peers`]
//! pass. Each importer's direct deps map is scanned for entries whose
//! resolved id starts with `file:` and points at another workspace
//! project; if that injected snapshot's children turn out to be a
//! subset of the target project's own direct deps, the importer's
//! entry is rewritten to `link:<rel>` so the install pass
//! materializes it as a symlink instead of a copy. Snapshots that
//! become unreachable after the rewrite are pruned from the graph so
//! they don't surface in the lockfile.
//!
//! Pacquet's lockfile importer-version writer does not yet support
//! `file:<workspace>` direct-dep entries — when an importer's
//! `direct_dependencies_by_alias` still carries an injected workspace
//! depPath after this pass, the lockfile writer panics in
//! `importer_dep_version`. In practice this pass is what keeps the
//! install from hitting that panic on every `dependenciesMeta.injected`
//! workspace edge; with `dedupeInjectedDeps: false`, an injected
//! workspace dep whose children don't subset still trips the writer.

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use pacquet_deps_path::DepPath;

use crate::dependencies_graph::DependenciesGraph;

/// Per-importer direct deps map keyed by lockfile importer id (`"."`
/// for the root, POSIX-relative path for siblings).
pub type DirectByImporter = BTreeMap<String, BTreeMap<String, DepPath>>;

/// Dedupe injected workspace deps across importers, mutating both
/// `direct_by_importer` (rewriting `file:` direct deps to `link:`)
/// and `graph` (dropping snapshots no importer reaches).
///
/// `importer_root_dirs` maps each importer id (`"."` for the root,
/// POSIX-relative for siblings) to its absolute project dir; the
/// caller already has this from the resolve loop.
pub fn dedupe_injected_deps(
    graph: &mut DependenciesGraph,
    direct_by_importer: &mut DirectByImporter,
    importer_root_dirs: &BTreeMap<String, PathBuf>,
    lockfile_dir: &Path,
) {
    let workspace_project_ids: HashSet<String> = importer_root_dirs.keys().cloned().collect();
    let dedupe_map = build_dedupe_map(graph, direct_by_importer, &workspace_project_ids);
    if dedupe_map.is_empty() {
        return;
    }
    apply_dedupe_map(&dedupe_map, direct_by_importer, importer_root_dirs, lockfile_dir);
    prune_unreachable(graph, direct_by_importer);
}

/// `importer_id → alias → target_project_id` — the per-importer set of
/// aliases that get rewritten, with the workspace project they redirect
/// to.
type DedupeMap = BTreeMap<String, BTreeMap<String, String>>;

fn build_dedupe_map(
    graph: &DependenciesGraph,
    direct_by_importer: &DirectByImporter,
    workspace_project_ids: &HashSet<String>,
) -> DedupeMap {
    let mut dedupe_map: DedupeMap = BTreeMap::new();
    for (importer_id, direct) in direct_by_importer {
        let mut deduped: BTreeMap<String, String> = BTreeMap::new();
        for (alias, dep_path) in direct {
            let Some(node) = graph.get(dep_path) else { continue };
            let Some(target_project_id) = injected_workspace_target(node, workspace_project_ids)
            else {
                continue;
            };
            // The injected dep can be rewritten to a symlink only when
            // every one of its children is also present (as the same
            // depPath) on the target project's own direct deps. The
            // empty set is trivially a subset.
            let target_direct = direct_by_importer.get(&target_project_id);
            let children_match = node.children.iter().all(|(child_alias, child_dep_path)| {
                target_direct.and_then(|map| map.get(child_alias)) == Some(child_dep_path)
            });
            if !children_match {
                continue;
            }
            deduped.insert(alias.clone(), target_project_id);
        }
        if !deduped.is_empty() {
            dedupe_map.insert(importer_id.clone(), deduped);
        }
    }
    dedupe_map
}

/// Return the workspace project id (lockfile importer key) this node
/// is an injected reference to, or `None` if it isn't a `file:`
/// pointing at a known workspace project.
fn injected_workspace_target(
    node: &crate::dependencies_graph::DependenciesGraphNode,
    workspace_project_ids: &HashSet<String>,
) -> Option<String> {
    let id = node.resolved_package_id.strip_prefix("file:")?;
    workspace_project_ids.contains(id).then(|| id.to_string())
}

fn apply_dedupe_map(
    dedupe_map: &DedupeMap,
    direct_by_importer: &mut DirectByImporter,
    importer_root_dirs: &BTreeMap<String, PathBuf>,
    lockfile_dir: &Path,
) {
    for (importer_id, aliases) in dedupe_map {
        let Some(source_root) = importer_root_dirs.get(importer_id) else { continue };
        let Some(direct) = direct_by_importer.get_mut(importer_id) else { continue };
        for (alias, target_project_id) in aliases {
            let Some(target_root) = importer_root_dirs.get(target_project_id) else { continue };
            let link_dep_path = make_link_dep_path(source_root, target_root, lockfile_dir);
            direct.insert(alias.clone(), link_dep_path);
        }
    }
}

/// Build the `link:<rel>` depPath payload, mirroring pnpm's
/// [`link:${normalize(path.relative(id, dedupedProjectId))}`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/dedupeInjectedDeps.ts#L98).
/// `id` and `dedupedProjectId` upstream are lockfile importer keys
/// (POSIX-relative paths from the lockfile dir), which pacquet stores
/// as absolute project dirs; resolving the relative path through the
/// filesystem layer keeps Windows path separators correct.
fn make_link_dep_path(source_root: &Path, target_root: &Path, lockfile_dir: &Path) -> DepPath {
    let rel =
        pathdiff::diff_paths(target_root, source_root).unwrap_or_else(|| target_root.to_path_buf());
    let rendered = rel.to_string_lossy().replace('\\', "/");
    let rel_posix = if rendered.is_empty() || rendered == "." {
        // Source and target match: emit a path relative to the lockfile
        // dir so the resulting symlink still resolves to a real
        // directory. In practice this branch is unreachable —
        // `injected_workspace_target` only matches when the importer's
        // direct dep points at a different workspace project — but the
        // fallback keeps the helper total.
        let fallback = pathdiff::diff_paths(target_root, lockfile_dir)
            .unwrap_or_else(|| target_root.to_path_buf());
        fallback.to_string_lossy().replace('\\', "/")
    } else {
        rendered
    };
    DepPath::from(format!("link:{rel_posix}"))
}

/// Remove graph nodes no importer reaches after dedupe, mirroring
/// pnpm's [`pruneSharedLockfile`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/pruner/src/index.ts).
/// The deduped `file:` snapshot would otherwise remain in
/// `merged_graph` and surface in `pnpm-lock.yaml` as an orphan.
fn prune_unreachable(graph: &mut DependenciesGraph, direct_by_importer: &DirectByImporter) {
    let mut reachable: HashSet<DepPath> = HashSet::new();
    let mut stack: Vec<DepPath> =
        direct_by_importer.values().flat_map(|direct| direct.values().cloned()).collect();
    while let Some(dep_path) = stack.pop() {
        if !reachable.insert(dep_path.clone()) {
            continue;
        }
        let Some(node) = graph.get(&dep_path) else { continue };
        for child in node.children.values() {
            if !reachable.contains(child) {
                stack.push(child.clone());
            }
        }
    }
    graph.retain(|key, _| reachable.contains(key));
}

#[cfg(test)]
mod tests {
    use super::{DirectByImporter, dedupe_injected_deps};
    use crate::dependencies_graph::{DependenciesGraph, DependenciesGraphNode};
    use pacquet_deps_path::DepPath;
    use pacquet_lockfile::{DirectoryResolution, LockfileResolution};
    use pacquet_resolving_resolver_base::{PkgResolutionId, ResolveResult};
    use std::collections::{BTreeMap, HashSet};
    use std::path::PathBuf;

    fn make_node(id: &str, children: BTreeMap<String, DepPath>) -> DependenciesGraphNode {
        DependenciesGraphNode {
            dep_path: DepPath::from(id.to_string()),
            resolved_package_id: id.to_string(),
            resolve_result: std::sync::Arc::new(ResolveResult {
                id: PkgResolutionId::from(id.to_string()),
                name_ver: None,
                latest: None,
                published_at: None,
                manifest: None,
                resolution: LockfileResolution::Directory(DirectoryResolution {
                    directory: "stub".to_string(),
                }),
                resolved_via: "workspace".to_string(),
                normalized_bare_specifier: None,
                alias: None,
                policy_violation: None,
            }),
            children,
            peer_dependencies: BTreeMap::new(),
            transitive_peer_dependencies: HashSet::new(),
            resolved_peer_names: HashSet::new(),
            depth: 0,
            installable: true,
            is_pure: true,
            optional: false,
        }
    }

    /// Two-project workspace: `project-2` injects `project-1`. The
    /// injected snapshot has no children, so the subset check trivially
    /// holds and the importer entry rewrites to a `link:`, with the
    /// orphaned `file:` snapshot pruned from the graph.
    #[test]
    fn rewrites_childless_injected_dep_to_link() {
        let lockfile_dir = PathBuf::from("/ws");
        let p1_root = lockfile_dir.join("project-1");
        let p2_root = lockfile_dir.join("project-2");

        let mut graph: DependenciesGraph = std::collections::HashMap::new();
        let file_path = DepPath::from("file:project-1".to_string());
        graph.insert(file_path.clone(), make_node("file:project-1", BTreeMap::new()));

        let mut direct: DirectByImporter = BTreeMap::new();
        direct.insert("project-1".to_string(), BTreeMap::new());
        direct.insert(
            "project-2".to_string(),
            BTreeMap::from([("project-1".to_string(), file_path)]),
        );

        let mut roots = BTreeMap::new();
        roots.insert("project-1".to_string(), p1_root);
        roots.insert("project-2".to_string(), p2_root);

        dedupe_injected_deps(&mut graph, &mut direct, &roots, &lockfile_dir);

        let after = direct.get("project-2").unwrap().get("project-1").unwrap();
        assert_eq!(after.as_str(), "link:../project-1");
        assert!(graph.is_empty(), "unreachable file: snapshot should be pruned");
    }

    /// When the injected snapshot's child set is not a subset of the
    /// target project's direct deps (different version reached
    /// transitively), the dedupe must bail and the `file:` direct dep
    /// must remain in place.
    #[test]
    fn leaves_injected_dep_when_children_differ() {
        let lockfile_dir = PathBuf::from("/ws");
        let p1_root = lockfile_dir.join("project-1");
        let p2_root = lockfile_dir.join("project-2");

        let lib_v1 = DepPath::from("lib@1.0.0".to_string());
        let lib_v2 = DepPath::from("lib@2.0.0".to_string());

        let mut graph: DependenciesGraph = std::collections::HashMap::new();
        graph.insert(lib_v1.clone(), make_node("lib@1.0.0", BTreeMap::new()));
        graph.insert(lib_v2.clone(), make_node("lib@2.0.0", BTreeMap::new()));
        let injected = DepPath::from("file:project-1".to_string());
        graph.insert(
            injected.clone(),
            make_node("file:project-1", BTreeMap::from([("lib".to_string(), lib_v2)])),
        );

        let mut direct: DirectByImporter = BTreeMap::new();
        direct.insert("project-1".to_string(), BTreeMap::from([("lib".to_string(), lib_v1)]));
        direct
            .insert("project-2".to_string(), BTreeMap::from([("project-1".to_string(), injected)]));

        let mut roots = BTreeMap::new();
        roots.insert("project-1".to_string(), p1_root);
        roots.insert("project-2".to_string(), p2_root);

        dedupe_injected_deps(&mut graph, &mut direct, &roots, &lockfile_dir);

        let after = direct.get("project-2").unwrap().get("project-1").unwrap();
        assert_eq!(after.as_str(), "file:project-1");
    }

    /// The subset check holds when the injected snapshot's children all
    /// match the target project's direct-deps map by alias and depPath
    /// — the rewrite goes through even though the snapshot has
    /// non-empty children.
    #[test]
    fn rewrites_when_children_subset_of_target_direct_deps() {
        let lockfile_dir = PathBuf::from("/ws");
        let p1_root = lockfile_dir.join("project-1");
        let p2_root = lockfile_dir.join("project-2");

        let lib = DepPath::from("lib@1.0.0".to_string());

        let mut graph: DependenciesGraph = std::collections::HashMap::new();
        graph.insert(lib.clone(), make_node("lib@1.0.0", BTreeMap::new()));
        let injected = DepPath::from("file:project-1".to_string());
        graph.insert(
            injected.clone(),
            make_node("file:project-1", BTreeMap::from([("lib".to_string(), lib.clone())])),
        );

        let mut direct: DirectByImporter = BTreeMap::new();
        direct.insert("project-1".to_string(), BTreeMap::from([("lib".to_string(), lib.clone())]));
        direct.insert(
            "project-2".to_string(),
            BTreeMap::from([("project-1".to_string(), injected.clone())]),
        );

        let mut roots = BTreeMap::new();
        roots.insert("project-1".to_string(), p1_root);
        roots.insert("project-2".to_string(), p2_root);

        dedupe_injected_deps(&mut graph, &mut direct, &roots, &lockfile_dir);

        let after = direct.get("project-2").unwrap().get("project-1").unwrap();
        assert_eq!(after.as_str(), "link:../project-1");
        // `lib` is still reachable through project-1's direct deps.
        assert!(graph.contains_key(&lib));
        // The injected snapshot is orphaned and gets pruned.
        assert!(!graph.contains_key(&injected));
    }

    /// `file:` deps that point outside the workspace (a tarball-shaped
    /// local file dep, not a workspace project) must not be touched.
    #[test]
    fn ignores_non_workspace_file_deps() {
        let lockfile_dir = PathBuf::from("/ws");
        let mut graph: DependenciesGraph = std::collections::HashMap::new();
        let tarball = DepPath::from("file:vendor/some-tarball.tgz".to_string());
        graph.insert(tarball.clone(), make_node("file:vendor/some-tarball.tgz", BTreeMap::new()));

        let mut direct: DirectByImporter = BTreeMap::new();
        direct.insert(".".to_string(), BTreeMap::from([("some-tarball".to_string(), tarball)]));

        let mut roots = BTreeMap::new();
        roots.insert(".".to_string(), lockfile_dir.clone());

        dedupe_injected_deps(&mut graph, &mut direct, &roots, &lockfile_dir);

        let after = direct.get(".").unwrap().get("some-tarball").unwrap();
        assert_eq!(after.as_str(), "file:vendor/some-tarball.tgz");
    }
}
