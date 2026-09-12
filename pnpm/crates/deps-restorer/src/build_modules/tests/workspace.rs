#[cfg(unix)]
use super::{
    super::{BuildModules, RebuildOptions},
    TEST_LOGGED_METHODS, create_marker_pkg, key, policy_from_specs, root_importers,
};
#[cfg(unix)]
use crate::{SkippedSnapshots, VirtualStoreLayout};
#[cfg(unix)]
use pnpm_config::PackageImportMethod;
#[cfg(unix)]
use pnpm_executor::ScriptsPrependNodePath;
#[cfg(unix)]
use pnpm_lockfile::SnapshotEntry;
#[cfg(unix)]
use pnpm_reporter::SilentReporter;
#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use tempfile::tempdir;

/// A `pacquet rebuild <pkg>` with a selection runs scripts ONLY for the
/// selected package, even with the side-effects cache disabled (so its
/// `is_built` short-circuit cannot fire). Both packages are allowed to
/// build, so without the rebuild-selection gate `zzz` would also run.
#[cfg(unix)]
#[test]
fn rebuild_selection_runs_only_selected_scripts() {
    let snapshots = HashMap::from([
        (key("aaa", "1.0.0"), SnapshotEntry::default()),
        (key("zzz", "1.0.0"), SnapshotEntry::default()),
    ]);
    let importers = root_importers(&[("aaa", "1.0.0"), ("zzz", "1.0.0")]);
    let policy = policy_from_specs([("aaa", true), ("zzz", true)], false);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    let aaa_dir = create_marker_pkg(virtual_store_dir.path(), &key("aaa", "1.0.0"));
    let zzz_dir = create_marker_pkg(virtual_store_dir.path(), &key("zzz", "1.0.0"));

    let rebuild = RebuildOptions {
        selected_names: Some(std::iter::once("aaa".to_string()).collect()),
        pending_projects: Vec::new(),
    };

    BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pnpm_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        importers: &importers,
        packages: None,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        requires_build_by_snapshot: None,
        engine_name: None,
        side_effects_cache: false,
        side_effects_cache_write: false,
        shared_side_effects_publisher: None,
        store_dir: None,
        store_index_writer: None,
        patches: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        shell_emulator: false,
        extra_env: &HashMap::new(),
        user_agent: "pnpm/test",
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_roots_by_key: None,
        gather_ancestor_bin_paths: false,
        frozen_store: false,
        ignore_scripts: false,
        import_method: PackageImportMethod::Auto,
        logged_methods: &TEST_LOGGED_METHODS,
        rebuild: Some(&rebuild),
    }
    .run::<SilentReporter>()
    .expect("rebuild runs");

    assert!(aaa_dir.join("built-marker").exists(), "the selected package's script ran");
    assert!(
        !zzz_dir.join("built-marker").exists(),
        "the non-selected package's script must not run",
    );
}
