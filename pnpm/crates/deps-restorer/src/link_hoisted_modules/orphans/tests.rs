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

/// Links an empty plan for the root and `packages/foo`, with the root's
/// modules directory at `root_modules_dir`.
fn link_empty_plan(lockfile_dir: &Path, root_modules_dir: &str) {
    let project_dir = lockfile_dir.join("packages/foo");
    let hierarchy = BTreeMap::from([
        (lockfile_dir.to_path_buf(), DepHierarchy::default()),
        (project_dir, DepHierarchy::default()),
    ]);
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
        previous_dirs: &BTreeSet::<PathBuf>::new(),
        hierarchy: &hierarchy,
        cas_paths_by_pkg_id: &HashMap::new(),
        link_options: &LinkBinsOptions::default(),
        modules_dirs: &ImporterModulesDirs::new(
            lockfile_dir,
            Path::new(root_modules_dir),
            OsStr::new("node_modules"),
        ),
    };
    link_hoisted_modules::<SilentReporter>(&opts).expect("linker succeeds");
}

#[test]
fn a_modules_dir_holding_a_project_keeps_the_project() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lockfile_dir = tmp.path().join("repo");
    write_package(&lockfile_dir.join("packages/foo"));
    write_package(&lockfile_dir.join("packages/stray"));

    link_empty_plan(&lockfile_dir, "packages");

    assert!(lockfile_dir.join("packages/foo/package.json").exists());
    assert!(!lockfile_dir.join("packages/stray").exists());
    assert!(lockfile_dir.join("packages/.ignored/stray/package.json").exists());
}

#[test]
fn a_modules_dir_that_is_not_inside_its_importer_is_not_scanned() {
    for root_modules_dir in [".", "..", "../outside"] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lockfile_dir = tmp.path().join("repo");
        let modules_dir = pnpm_fs::lexical_normalize(&lockfile_dir.join(root_modules_dir));
        let stray = modules_dir.join("stray");
        write_package(&stray);

        link_empty_plan(&lockfile_dir, root_modules_dir);

        eprintln!("modulesDir: {root_modules_dir}");
        assert!(stray.join("package.json").exists());
    }
}
