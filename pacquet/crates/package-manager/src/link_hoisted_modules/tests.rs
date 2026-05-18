//! Tests for [`super::link_hoisted_modules`].
//!
//! The linker is sync and consumes a fully-populated CAS index.
//! Each test plants synthetic CAS files in a tempdir, builds a
//! graph + hierarchy by hand, and asserts the on-disk tree
//! after `link_hoisted_modules` runs.

use super::{
    CasPathsByPkgId, LinkHoistedModulesError, LinkHoistedModulesOpts, link_hoisted_modules,
};
use crate::{DepHierarchy, DependenciesGraph, DependenciesGraphNode};
use pacquet_config::PackageImportMethod;
use pacquet_lockfile::{DirectoryResolution, LockfileResolution, PkgIdWithPatchHash};
use pacquet_modules_yaml::DepPath;
use pacquet_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
};

fn sample_resolution() -> LockfileResolution {
    DirectoryResolution { directory: "/dev/null/stub".to_string() }.into()
}

/// Build a minimal graph node at `dir`. The walker would do
/// this through `lockfile_to_hoisted_dep_graph`; tests build it
/// directly so the linker can be exercised without a lockfile.
fn make_node(alias: &str, dep_path: &str, pkg_id: &str, dir: PathBuf) -> DependenciesGraphNode {
    let modules = dir.parent().expect("dir has parent").to_path_buf();
    DependenciesGraphNode {
        alias: Some(alias.to_string()),
        dep_path: DepPath::from(dep_path.to_string()),
        pkg_id_with_patch_hash: PkgIdWithPatchHash::from(pkg_id),
        dir,
        modules,
        children: BTreeMap::new(),
        name: alias.to_string(),
        version: "1.0.0".to_string(),
        optional: false,
        optional_dependencies: BTreeSet::new(),
        has_bin: false,
        has_bundled_dependencies: false,
        patch: None,
        resolution: sample_resolution(),
    }
}

/// Write a synthetic CAS entry under `cas_root/<pkg_id>/file`
/// containing `contents`, and return the per-pkg cas_paths map
/// keyed on the package's relative archive path.
fn plant_cas_file(
    cas_root: &Path,
    pkg_id: &str,
    rel_path: &str,
    contents: &[u8],
) -> HashMap<String, PathBuf> {
    let pkg_dir = cas_root.join(pkg_id);
    let file_path = pkg_dir.join(rel_path);
    fs::create_dir_all(file_path.parent().expect("file has parent"))
        .expect("create CAS file parent");
    fs::write(&file_path, contents).expect("write CAS file");
    let mut paths = HashMap::new();
    paths.insert(rel_path.to_string(), file_path);
    paths
}

/// Combine two cas_paths maps into one for tests that emit
/// multiple files per package.
fn plant_package(
    cas_root: &Path,
    pkg_id: &str,
    files: &[(&str, &[u8])],
) -> HashMap<String, PathBuf> {
    let mut combined = HashMap::new();
    for (rel, contents) in files {
        let single = plant_cas_file(cas_root, pkg_id, rel, contents);
        combined.extend(single);
    }
    combined
}

/// Build a `(graph, hierarchy, cas_paths_by_pkg_id)` for a single
/// flat install: `lockfile_dir/node_modules/<alias>` per entry.
/// `entries: &[(alias, dep_path, pkg_id, files)]`.
fn flat_layout(
    lockfile_dir: &Path,
    cas_root: &Path,
    entries: &[(&str, &str, &str, &[(&str, &[u8])])],
) -> (DependenciesGraph, BTreeMap<PathBuf, DepHierarchy>, CasPathsByPkgId) {
    let modules = lockfile_dir.join("node_modules");
    let mut graph = DependenciesGraph::new();
    let mut hierarchy_children = BTreeMap::new();
    let mut cas_paths = CasPathsByPkgId::new();
    for (alias, dep_path, pkg_id, files) in entries {
        let dir = modules.join(alias);
        graph.insert(dir.clone(), make_node(alias, dep_path, pkg_id, dir.clone()));
        hierarchy_children.insert(dir, DepHierarchy::default());
        cas_paths.insert(PkgIdWithPatchHash::from(*pkg_id), plant_package(cas_root, pkg_id, files));
    }
    let mut hierarchy = BTreeMap::new();
    hierarchy.insert(lockfile_dir.to_path_buf(), DepHierarchy(hierarchy_children));
    (graph, hierarchy, cas_paths)
}

/// Each node in `graph` should produce a populated directory
/// containing the planted CAS files. Single-package smoke test.
#[test]
fn import_pass_creates_package_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cas_root = tmp.path().join("cas");
    let lockfile_dir = tmp.path().join("repo");
    let (graph, hierarchy, cas_paths) = flat_layout(
        &lockfile_dir,
        &cas_root,
        &[("a", "a@1.0.0", "a@1.0.0", &[("package/index.js", b"module.exports = 1;")])],
    );

    let logged = AtomicU8::new(0);
    let opts = LinkHoistedModulesOpts {
        graph: &graph,
        prev_graph: None,
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &cas_paths,
        import_method: PackageImportMethod::Auto,
        logged_methods: &logged,
        requester: lockfile_dir.to_str().expect("requester"),
    };
    link_hoisted_modules::<SilentReporter>(&opts).expect("linker succeeds");

    let installed = lockfile_dir.join("node_modules").join("a").join("package").join("index.js");
    assert!(installed.exists(), "imported file at {installed:?}");
    assert_eq!(fs::read(&installed).unwrap(), b"module.exports = 1;");
}

/// A directory present in `prev_graph` but not in `graph` is
/// rimraf'd before the import pass. The directory and its
/// contents are gone after the linker runs.
#[test]
fn orphan_directory_is_removed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cas_root = tmp.path().join("cas");
    let lockfile_dir = tmp.path().join("repo");
    let modules = lockfile_dir.join("node_modules");

    // Plant a stale directory from the "previous" install.
    let orphan_dir = modules.join("orphan");
    fs::create_dir_all(&orphan_dir).expect("create orphan dir");
    fs::write(orphan_dir.join("stale.txt"), b"old data").expect("write stale file");
    assert!(orphan_dir.exists());

    // prev_graph: a, orphan ; graph: a only.
    let mut prev_graph = DependenciesGraph::new();
    prev_graph.insert(modules.join("a"), make_node("a", "a@1.0.0", "a@1.0.0", modules.join("a")));
    prev_graph.insert(
        orphan_dir.clone(),
        make_node("orphan", "orphan@1.0.0", "orphan@1.0.0", orphan_dir.clone()),
    );

    let (graph, hierarchy, cas_paths) = flat_layout(
        &lockfile_dir,
        &cas_root,
        &[("a", "a@1.0.0", "a@1.0.0", &[("package/index.js", b"module.exports = 1;")])],
    );

    let logged = AtomicU8::new(0);
    let opts = LinkHoistedModulesOpts {
        graph: &graph,
        prev_graph: Some(&prev_graph),
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &cas_paths,
        import_method: PackageImportMethod::Auto,
        logged_methods: &logged,
        requester: lockfile_dir.to_str().expect("requester"),
    };
    link_hoisted_modules::<SilentReporter>(&opts).expect("linker succeeds");

    assert!(!orphan_dir.exists(), "orphan rimraf'd: {orphan_dir:?}");
    assert!(modules.join("a").join("package").join("index.js").exists(), "a is imported");
}

/// A nested hierarchy materializes the inner package under
/// `<outer>/node_modules/<inner>`. Mirrors the version-conflict
/// case where Slice 4's walker nests a losing version.
#[test]
fn nested_hierarchy_materializes_inner_node_modules() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cas_root = tmp.path().join("cas");
    let lockfile_dir = tmp.path().join("repo");
    let modules = lockfile_dir.join("node_modules");

    let outer_dir = modules.join("outer");
    let inner_dir = outer_dir.join("node_modules").join("inner");

    let mut graph = DependenciesGraph::new();
    graph.insert(
        outer_dir.clone(),
        make_node("outer", "outer@1.0.0", "outer@1.0.0", outer_dir.clone()),
    );
    graph.insert(
        inner_dir.clone(),
        make_node("inner", "inner@2.0.0", "inner@2.0.0", inner_dir.clone()),
    );

    let mut inner_children = BTreeMap::new();
    inner_children.insert(inner_dir.clone(), DepHierarchy::default());
    let mut outer_children = BTreeMap::new();
    outer_children.insert(outer_dir.clone(), DepHierarchy(inner_children));
    let mut hierarchy = BTreeMap::new();
    hierarchy.insert(lockfile_dir.clone(), DepHierarchy(outer_children));

    let mut cas_paths = CasPathsByPkgId::new();
    cas_paths.insert(
        PkgIdWithPatchHash::from("outer@1.0.0"),
        plant_package(&cas_root, "outer@1.0.0", &[("package/outer.js", b"// outer")]),
    );
    cas_paths.insert(
        PkgIdWithPatchHash::from("inner@2.0.0"),
        plant_package(&cas_root, "inner@2.0.0", &[("package/inner.js", b"// inner")]),
    );

    let logged = AtomicU8::new(0);
    let opts = LinkHoistedModulesOpts {
        graph: &graph,
        prev_graph: None,
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &cas_paths,
        import_method: PackageImportMethod::Auto,
        logged_methods: &logged,
        requester: lockfile_dir.to_str().expect("requester"),
    };
    link_hoisted_modules::<SilentReporter>(&opts).expect("linker succeeds");

    assert!(outer_dir.join("package").join("outer.js").exists(), "outer imported");
    assert!(inner_dir.join("package").join("inner.js").exists(), "nested inner imported");
}

/// Missing CAS for a required package surfaces as
/// `LinkHoistedModulesError::MissingCasPaths` instead of crashing.
#[test]
fn missing_cas_for_required_dep_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lockfile_dir = tmp.path().join("repo");
    let modules = lockfile_dir.join("node_modules");

    let dir = modules.join("a");
    let mut graph = DependenciesGraph::new();
    graph.insert(dir.clone(), make_node("a", "a@1.0.0", "a@1.0.0", dir.clone()));

    let mut hierarchy_children = BTreeMap::new();
    hierarchy_children.insert(dir.clone(), DepHierarchy::default());
    let mut hierarchy = BTreeMap::new();
    hierarchy.insert(lockfile_dir.clone(), DepHierarchy(hierarchy_children));

    // No CAS entries planted at all.
    let cas_paths = CasPathsByPkgId::new();

    let logged = AtomicU8::new(0);
    let opts = LinkHoistedModulesOpts {
        graph: &graph,
        prev_graph: None,
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &cas_paths,
        import_method: PackageImportMethod::Auto,
        logged_methods: &logged,
        requester: lockfile_dir.to_str().expect("requester"),
    };
    let err = link_hoisted_modules::<SilentReporter>(&opts).expect_err("required dep needs CAS");
    match err {
        LinkHoistedModulesError::MissingCasPaths { pkg_id_with_patch_hash, .. } => {
            assert_eq!(pkg_id_with_patch_hash, PkgIdWithPatchHash::from("a@1.0.0"));
        }
        other => panic!("expected MissingCasPaths, got {other:?}"),
    }
}

/// Missing CAS for an optional package is silently skipped —
/// the directory isn't created and no error surfaces. Mirrors
/// upstream's `if (depNode.optional) return` on fetch failure.
#[test]
fn missing_cas_for_optional_dep_skips_silently() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lockfile_dir = tmp.path().join("repo");
    let modules = lockfile_dir.join("node_modules");

    let dir = modules.join("a");
    let mut node = make_node("a", "a@1.0.0", "a@1.0.0", dir.clone());
    node.optional = true;
    let mut graph = DependenciesGraph::new();
    graph.insert(dir.clone(), node);

    let mut hierarchy_children = BTreeMap::new();
    hierarchy_children.insert(dir.clone(), DepHierarchy::default());
    let mut hierarchy = BTreeMap::new();
    hierarchy.insert(lockfile_dir.clone(), DepHierarchy(hierarchy_children));

    let cas_paths = CasPathsByPkgId::new();

    let logged = AtomicU8::new(0);
    let opts = LinkHoistedModulesOpts {
        graph: &graph,
        prev_graph: None,
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &cas_paths,
        import_method: PackageImportMethod::Auto,
        logged_methods: &logged,
        requester: lockfile_dir.to_str().expect("requester"),
    };
    link_hoisted_modules::<SilentReporter>(&opts).expect("optional skips silently");

    assert!(!dir.exists(), "optional dir with no CAS not created");
}

/// `prev_graph: None` is a no-op for orphan removal. Sanity check
/// that fresh installs (no prior lockfile) don't fail on the
/// orphan pass.
#[test]
fn no_prev_graph_skips_orphan_pass() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cas_root = tmp.path().join("cas");
    let lockfile_dir = tmp.path().join("repo");

    let (graph, hierarchy, cas_paths) = flat_layout(
        &lockfile_dir,
        &cas_root,
        &[("a", "a@1.0.0", "a@1.0.0", &[("package/index.js", b"hi")])],
    );

    let logged = AtomicU8::new(0);
    let opts = LinkHoistedModulesOpts {
        graph: &graph,
        prev_graph: None,
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &cas_paths,
        import_method: PackageImportMethod::Auto,
        logged_methods: &logged,
        requester: lockfile_dir.to_str().expect("requester"),
    };
    link_hoisted_modules::<SilentReporter>(&opts).expect("linker succeeds without prev_graph");

    assert!(lockfile_dir.join("node_modules").join("a").join("package").join("index.js").exists());
}

/// Orphan removal tolerates errors silently — matches upstream's
/// `tryRemoveDir` EPERM/EBUSY swallowing. We can't easily provoke
/// a permission error in a test, but a non-existent directory in
/// `prev_graph` (e.g. one already removed by another process)
/// must not error.
#[test]
fn orphan_already_removed_is_tolerated() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cas_root = tmp.path().join("cas");
    let lockfile_dir = tmp.path().join("repo");
    let modules = lockfile_dir.join("node_modules");

    // prev_graph references a directory that doesn't exist on
    // disk — a prior install pass already removed it.
    let phantom_orphan = modules.join("phantom");
    let mut prev_graph = DependenciesGraph::new();
    prev_graph.insert(
        phantom_orphan.clone(),
        make_node("phantom", "phantom@1.0.0", "phantom@1.0.0", phantom_orphan.clone()),
    );

    let (graph, hierarchy, cas_paths) = flat_layout(
        &lockfile_dir,
        &cas_root,
        &[("a", "a@1.0.0", "a@1.0.0", &[("package/index.js", b"hi")])],
    );

    let logged = AtomicU8::new(0);
    let opts = LinkHoistedModulesOpts {
        graph: &graph,
        prev_graph: Some(&prev_graph),
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &cas_paths,
        import_method: PackageImportMethod::Auto,
        logged_methods: &logged,
        requester: lockfile_dir.to_str().expect("requester"),
    };
    link_hoisted_modules::<SilentReporter>(&opts).expect("phantom orphan tolerated");
}

/// A hierarchy entry whose directory has no matching graph node
/// surfaces as `MissingGraphNode` rather than being silently
/// skipped. Pinning fail-fast on internal inconsistency between
/// the hierarchy and graph — Slice 4's walker keeps the two in
/// sync, but a future bug there shouldn't yield a partial
/// install layout.
#[test]
fn hierarchy_entry_missing_from_graph_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lockfile_dir = tmp.path().join("repo");
    let modules = lockfile_dir.join("node_modules");
    let dir = modules.join("phantom");

    // Hierarchy references `phantom`, but the graph is empty.
    let mut hierarchy_children = BTreeMap::new();
    hierarchy_children.insert(dir.clone(), DepHierarchy::default());
    let mut hierarchy = BTreeMap::new();
    hierarchy.insert(lockfile_dir.clone(), DepHierarchy(hierarchy_children));

    let graph = DependenciesGraph::new();
    let cas_paths = CasPathsByPkgId::new();
    let logged = AtomicU8::new(0);
    let opts = LinkHoistedModulesOpts {
        graph: &graph,
        prev_graph: None,
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &cas_paths,
        import_method: PackageImportMethod::Auto,
        logged_methods: &logged,
        requester: lockfile_dir.to_str().expect("requester"),
    };
    let err = link_hoisted_modules::<SilentReporter>(&opts).expect_err("inconsistency surfaces");
    match err {
        LinkHoistedModulesError::MissingGraphNode { dir: reported_dir } => {
            assert_eq!(reported_dir, dir);
        }
        other => panic!("expected MissingGraphNode, got {other:?}"),
    }
}
