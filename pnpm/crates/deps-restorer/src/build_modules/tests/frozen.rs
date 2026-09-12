#[cfg(unix)]
use super::{
    super::BuildModules, TEST_LOGGED_METHODS, create_postinstall_modifies_source_fixture, key,
    policy_from_specs, report_store_file_differences, root_importers, snapshot_regular_files,
};
use super::{frozen_backstop_run, gvs_layout};
#[cfg(unix)]
use crate::SkippedSnapshots;
use crate::VirtualStoreLayout;
#[cfg(unix)]
use pnpm_config::PackageImportMethod;
#[cfg(unix)]
use pnpm_executor::ScriptsPrependNodePath;
#[cfg(unix)]
use pnpm_lockfile::SnapshotEntry;
#[cfg(unix)]
use pnpm_reporter::SilentReporter;
#[cfg(unix)]
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::collections::HashMap;
use tempfile::tempdir;

/// Positive: under the global virtual store, a patched package whose
/// build output is missing from the read-only store refuses up front
/// with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` rather than crashing on a
/// raw `EROFS` when `apply_patch_to_dir` tries to write into the store.
#[test]
fn frozen_store_gvs_patch_not_seeded_refuses() {
    let store_dir = tempdir().expect("create temp dir");
    let layout = gvs_layout(store_dir.path());

    let err = frozen_backstop_run(layout, true, false)
        .expect_err("a missing patched build under a frozen GVS store must refuse up front");
    assert!(
        matches!(err, crate::build_modules::BuildModulesError::FrozenStoreNeedsBuild { .. }),
        "expected FrozenStoreNeedsBuild, got {err:?}",
    );
}
/// An optional patched snapshot must not block the install: a build or
/// patch failure on an optional dependency is non-fatal at runtime, so
/// the backstop skips its build (emitting the skipped-optional log)
/// instead of refusing.
#[test]
fn frozen_store_gvs_optional_not_seeded_skips() {
    let store_dir = tempdir().expect("create temp dir");
    let layout = gvs_layout(store_dir.path());

    let ignored = frozen_backstop_run(layout, true, true)
        .expect("an optional un-seeded build must be skipped, not refused")
        .ignored_builds;
    assert!(ignored.is_empty(), "no scripts to ignore for a patched-only snapshot: {ignored:?}");
}
/// Negative control: the same patched, un-seeded snapshot does NOT trip
/// the backstop when `frozen_store` is off — proving the flag is
/// load-bearing. With no fixture on disk the run no-ops at the
/// `!pkg_dir.exists()` guard, so it returns `Ok` rather than attempting
/// to apply the (absent) patch file.
#[test]
fn gvs_without_frozen_store_does_not_trip_backstop() {
    let store_dir = tempdir().expect("create temp dir");
    let layout = gvs_layout(store_dir.path());

    let ignored = frozen_backstop_run(layout, false, false)
        .expect("without frozen_store the backstop must not fire")
        .ignored_builds;
    assert!(ignored.is_empty(), "no scripts to ignore for a patched-only snapshot: {ignored:?}");
}
/// Negative control: under the legacy (non-GVS) layout, package
/// directories live under the writable project-local virtual store, so
/// the backstop is correctly inert even with `frozen_store` enabled —
/// builds and patches there never touch the read-only store.
#[test]
fn frozen_store_without_gvs_does_not_trip_backstop() {
    let virtual_store_dir = tempdir().expect("create temp dir");
    let layout = VirtualStoreLayout::legacy(
        virtual_store_dir.path(),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );

    let ignored = frozen_backstop_run(&layout, true, false)
        .expect("the non-GVS layout writes to the project store, so the backstop must not fire")
        .ignored_builds;
    assert!(ignored.is_empty(), "no scripts to ignore for a patched-only snapshot: {ignored:?}");
}
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn frozen_store_skips_side_effects_upload() {
    use pnpm_store_dir::{StoreDir, StoreIndexWriter};

    let pkg_key = key("@pnpm/postinstall-modifies-source", "1.0.0");
    let integrity_str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::PackageMetadata> =
        HashMap::from([(
            pkg_key.without_peer(),
            pnpm_lockfile::PackageMetadata {
                resolution: pnpm_lockfile::LockfileResolution::Registry(
                    pnpm_lockfile::RegistryResolution {
                        integrity: integrity_str.parse().expect("parse integrity"),
                        revision: None,
                    },
                ),
                version: None,
                engines: None,
                cpu: None,
                os: None,
                libc: None,
                deprecated: None,
                has_bin: None,
                prepare: None,
                bundled_dependencies: None,
                peer_dependencies: None,
                peer_dependencies_meta: None,
            },
        )]);
    let importers = root_importers(&[("@pnpm/postinstall-modifies-source", "1.0.0")]);
    let policy = policy_from_specs([], true);
    let side_effects_maps = crate::SideEffectsMapsBySnapshot::from([(
        pkg_key.clone(),
        std::sync::Arc::new(HashMap::new()),
    )]);

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");
    let (pkg_dir, _actual_mode) =
        create_postinstall_modifies_source_fixture(virtual_store_dir.path(), &pkg_key);
    let store_before = snapshot_regular_files(store_dir.root());
    let (writer, writer_task) = StoreIndexWriter::spawn_disabled();

    BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pnpm_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: Some(&packages),
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: Some(&side_effects_maps),
        requires_build_by_snapshot: None,
        engine_name: Some("darwin;arm64;node20"),
        side_effects_cache: true,
        side_effects_cache_write: true,
        shared_side_effects_publisher: None,
        store_dir: Some(&store_dir),
        store_index_writer: Some(&writer),
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
        frozen_store: true,
        ignore_scripts: false,
        import_method: PackageImportMethod::Auto,
        logged_methods: &TEST_LOGGED_METHODS,
        rebuild: None,
    }
    .run::<SilentReporter>()
    .expect("build modules must complete cleanly");

    drop(writer);
    writer_task.await.expect("await writer").expect("disabled writer succeeds");

    let generated_file = pkg_dir.join("generated.txt");
    let generated_file_exists = generated_file.exists();
    if !generated_file_exists {
        eprintln!("Expected generated file: {}", generated_file.display());
    }
    assert!(generated_file_exists, "postinstall must run outside the store");

    let store_after = snapshot_regular_files(store_dir.root());
    report_store_file_differences(&store_before, &store_after);
    assert_eq!(store_after, store_before);
}
