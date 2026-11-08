use crate::{
    DepHierarchy, DependenciesGraph, ImporterModulesDirs, LinkHoistedModulesOpts,
    link_hoisted_modules,
};
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
};

fn write_package(dir: &Path) {
    fs::create_dir_all(dir).expect("create package dir");
    fs::write(dir.join("package.json"), r#"{"name":"pkg","version":"1.0.0"}"#)
        .expect("write package.json");
}

/// Links an empty plan for a workspace of the root and a dependency-free
/// `packages/foo`, with the root's modules directory at `root_modules_dir`.
fn link_empty_plan(lockfile_dir: &Path, root_modules_dir: &str, previous_dirs: &BTreeSet<PathBuf>) {
    let hierarchy = BTreeMap::from([(lockfile_dir.to_path_buf(), DepHierarchy::default())]);
    let logged = AtomicU8::new(0);
    let opts = LinkHoistedModulesOpts {
        import: crate::PackageImportOptions {
            method: PackageImportMethod::Auto,
            logged_methods: &logged,
            swap_lock: None,
            requester: lockfile_dir.to_str().expect("requester"),
            isolate_mutable_sources: false,
        },
        dir_clone_cache: None,
        graph: &DependenciesGraph::new(),
        previous_dirs,
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &HashMap::new(),
        link_options: &LinkBinsOptions::default(),
        modules_dirs: &ImporterModulesDirs::new(
            lockfile_dir,
            Path::new(root_modules_dir),
            OsStr::new("node_modules"),
        )
        .with_importers([".", "packages/foo"]),
    };
    link_hoisted_modules::<SilentReporter>(&opts).expect("linker succeeds");
}

#[test]
fn a_modules_dir_holding_a_project_is_not_scanned() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lockfile_dir = tmp.path().join("repo");
    write_package(&lockfile_dir.join("packages/foo"));
    write_package(&lockfile_dir.join("packages/stray"));

    link_empty_plan(&lockfile_dir, "packages", &BTreeSet::new());

    assert!(lockfile_dir.join("packages/foo/package.json").exists());
    assert!(lockfile_dir.join("packages/stray/package.json").exists());
}

#[test]
fn a_modules_dir_that_is_not_inside_its_importer_is_not_scanned() {
    for root_modules_dir in [".", "..", "../outside"] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lockfile_dir = tmp.path().join("repo");
        let modules_dir = pnpm_fs::lexical_normalize(&lockfile_dir.join(root_modules_dir));
        let stray = modules_dir.join("stray");
        write_package(&stray);

        link_empty_plan(&lockfile_dir, root_modules_dir, &BTreeSet::new());

        eprintln!("modulesDir: {root_modules_dir}");
        assert!(stray.join("package.json").exists());
    }
}

#[test]
fn a_previous_dir_that_is_the_workspace_or_holds_a_project_is_kept() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lockfile_dir = tmp.path().join("repo");
    write_package(&lockfile_dir);
    write_package(&lockfile_dir.join("packages/foo"));
    let previous_dirs = BTreeSet::from([
        lockfile_dir.clone(),
        lockfile_dir.join("packages"),
        lockfile_dir.join("packages/foo"),
    ]);

    link_empty_plan(&lockfile_dir, "node_modules", &previous_dirs);

    assert!(lockfile_dir.join("package.json").exists());
    assert!(lockfile_dir.join("packages/foo/package.json").exists());
}
