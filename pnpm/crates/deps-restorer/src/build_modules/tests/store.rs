#[cfg(unix)]
use super::{
    super::BuildModules, TEST_LOGGED_METHODS, create_failing_postinstall_fixture,
    create_postinstall_modifies_source_fixture, key, policy_from_specs, root_importers, sha512_hex,
};
use crate::store_index_key_for_resolution;
#[cfg(unix)]
use crate::{RequiresBuildBySnapshot, SkippedSnapshots, VirtualStoreLayout};
#[cfg(unix)]
use pnpm_config::PackageImportMethod;
#[cfg(unix)]
use pnpm_executor::ScriptsPrependNodePath;
#[cfg(unix)]
use pnpm_lockfile::SnapshotEntry;
#[cfg(unix)]
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::{collections::HashMap, fs};
#[cfg(unix)]
use tempfile::tempdir;

#[test]
fn side_effects_key_for_git_hosted_tarball_matches_warm_lookup() {
    let integrity = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let pkg_id = "https://codeload.github.com/pnpm/pnpm/tar.gz/abcdef";
    let resolution = pnpm_lockfile::LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
        tarball: pkg_id.to_string(),
        integrity: Some(integrity.parse().expect("parse integrity")),
        revision: None,
        git_hosted: Some(true),
        path: None,
    });

    assert_eq!(
        store_index_key_for_resolution(&resolution, pkg_id, true),
        Some(pnpm_store_dir::git_hosted_store_index_key(pkg_id, true)),
    );
}
#[test]
fn side_effects_key_for_git_resolution_does_not_require_integrity() {
    let pkg_id = "git+file:///tmp/repo#abcdef";
    let resolution = pnpm_lockfile::LockfileResolution::Git(pnpm_lockfile::GitResolution {
        repo: "file:///tmp/repo".to_string(),
        commit: "abcdef".to_string(),
        integrity: None,
        path: None,
    });

    assert_eq!(
        store_index_key_for_resolution(&resolution, pkg_id, true),
        Some(pnpm_store_dir::git_hosted_store_index_key(pkg_id, true)),
    );
}
/// If a failed materialization left the slot without its manifest (a
/// stage-and-swap that failed mid-replace), the install must not finish
/// with a silently broken package: a non-optional snapshot surfaces a
/// hard error rather than falling through to a no-op "rebuild" over the
/// incomplete directory.
#[cfg(unix)]
#[test]
fn materialization_failure_on_incomplete_slot_is_fatal() {
    let pkg_key = key("@pnpm.e2e/postinstall-modifies-source", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::PackageMetadata> =
        HashMap::from([(
            pkg_key.without_peer(),
            pnpm_lockfile::PackageMetadata {
                resolution: pnpm_lockfile::LockfileResolution::Registry(
                    pnpm_lockfile::RegistryResolution {
                        integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                            .parse()
                            .expect("parse integrity"),
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
    let importers = root_importers(&[("@pnpm.e2e/postinstall-modifies-source", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    // A slot directory that exists but has lost its manifest — the state a
    // stage-and-swap failure would leave behind.
    let (pkg_dir, _mode) =
        create_postinstall_modifies_source_fixture(virtual_store_dir.path(), &pkg_key);
    fs::remove_file(pkg_dir.join("package.json")).expect("remove manifest");

    let engine = "darwin;arm64;node20";
    let dep_graph = crate::build_deps_graph(&snapshots, &packages);
    let mut state_cache = pnpm_graph_hasher::DepsStateCache::new();
    let expected_cache_key = pnpm_graph_hasher::calc_dep_state(
        &dep_graph,
        &mut state_cache,
        &pkg_key,
        &pnpm_graph_hasher::CalcDepStateOptions {
            engine_name: engine,
            patch_file_hash: None,
            include_dep_graph_hash: true,
        },
    );
    // Overlay points at a non-existent CAS blob, so materialization fails.
    let overlay = std::collections::HashMap::from([(
        "generated.txt".to_string(),
        virtual_store_dir.path().join("missing-cas-blob"),
    )]);
    let mut side_effects_maps = std::collections::HashMap::new();
    side_effects_maps.insert(
        pkg_key.clone(),
        std::sync::Arc::new(HashMap::from([(expected_cache_key, overlay)])),
    );
    // `requires_build` must be forced on: the gate is only reached for a
    // build candidate, and the manifest-less slot would otherwise probe as
    // not-requiring-build.
    let requires_build: RequiresBuildBySnapshot = HashMap::from([(pkg_key.clone(), true)]);

    let result = BuildModules {
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
        requires_build_by_snapshot: Some(&requires_build),
        engine_name: Some(engine),
        side_effects_cache: true,
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
        rebuild: None,
    }
    .run::<SilentReporter>();

    assert!(
        result.is_err(),
        "a non-optional package whose slot lost its manifest must fail the install, not finish broken",
    );
}
/// Negative pair: with `side_effects_cache = false`, even a
/// matching cache entry is ignored — the build runs.
#[cfg(unix)]
#[test]
fn side_effects_cache_disabled_bypasses_the_gate() {
    let pkg_key = key("@pnpm.e2e/failing-postinstall", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::PackageMetadata> =
        HashMap::new();
    let importers = root_importers(&[("@pnpm.e2e/failing-postinstall", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_failing_postinstall_fixture(virtual_store_dir.path(), &pkg_key);

    // Same overlay shape as the positive test, but the
    // `side_effects_cache: false` flag must short-circuit before
    // the lookup even runs.
    let mut overlay = std::collections::HashMap::new();
    overlay.insert("any-key".to_string(), std::collections::HashMap::new());
    let mut side_effects_maps = std::collections::HashMap::new();
    side_effects_maps.insert(pkg_key, std::sync::Arc::new(overlay));

    let err = BuildModules {
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
        rebuild: None,
    }
    .run::<SilentReporter>()
    .expect_err("with cache disabled, the failing postinstall must run and the install must fail");
    assert!(matches!(err, crate::build_modules::BuildModulesError::LifecycleScript(_)));
}
/// A postinstall script does not modify the original sources added
/// to the store.
///
/// After a successful postinstall, `BuildModules` re-CAFS the
/// built directory, diffs against the pristine `PackageFilesIndex.files`
/// row pre-seeded in the store, and queues a mutation so the row's
/// `side_effects[cache_key]` carries the post-build files that
/// differ from the base. The base CAS blob is left untouched — the
/// digest the store-index row holds for `index.js` matches the
/// pristine content, not the post-build content.
///
/// Unix-gated because the fixture uses `sh -c` semantics for the
/// `postinstall` script. Windows shell selection is exercised
/// separately by `pnpm_executor::select_shell`.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn write_path_populates_side_effects_row() {
    use pnpm_store_dir::{
        CafsFileInfo, HASH_ALGORITHM, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexWriter,
        store_index_key,
    };

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

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");
    let (_pkg_dir, actual_mode) =
        create_postinstall_modifies_source_fixture(virtual_store_dir.path(), &pkg_key);

    // Pre-seed the base PackageFilesIndex row that the WRITE
    // path will mutate. The base captures only `index.js`; the
    // postinstall creates `generated.txt` on top, so the diff's
    // `added` map should have exactly the `generated.txt` entry.
    let files_index_file = store_index_key(integrity_str, &pkg_key.without_peer().to_string());
    let mut base_files = HashMap::new();
    base_files.insert(
        "index.js".to_string(),
        CafsFileInfo {
            // The pristine content's actual digest is irrelevant
            // for this test — the WRITE path doesn't compare it
            // against on-disk CAS, just against the post-build
            // hashes from `add_files_from_dir`. So long as it
            // matches what `add_files_from_dir` will compute for
            // `module.exports = 'hi'\n`, the diff for `index.js`
            // stays empty (= no spurious entry in `added`).
            digest: sha512_hex(b"module.exports = 'hi'\n"),
            mode: actual_mode,
            size: b"module.exports = 'hi'\n".len() as u64,
            checked_at: None,
        },
    );
    let base_row = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        requires_prepare: None,
        algo: HASH_ALGORITHM.to_string(),
        files: base_files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    {
        let mut index = StoreIndex::open_in(&store_dir).expect("open index for seed");
        index
            .set_many(std::iter::once((files_index_file.clone(), base_row)))
            .expect("seed base row");
    }

    // Spawn the writer task once we've seeded the base row, so
    // the seed and the WRITE-path mutation don't race.
    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    let engine = "darwin;arm64;node20";
    let dep_graph = crate::build_deps_graph(&snapshots, &packages);
    let mut state_cache = pnpm_graph_hasher::DepsStateCache::new();
    let expected_cache_key = pnpm_graph_hasher::calc_dep_state(
        &dep_graph,
        &mut state_cache,
        &pkg_key,
        &pnpm_graph_hasher::CalcDepStateOptions {
            engine_name: engine,
            patch_file_hash: None,
            include_dep_graph_hash: true,
        },
    );

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
        side_effects_maps_by_snapshot: None,
        requires_build_by_snapshot: None,
        engine_name: Some(engine),
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
        frozen_store: false,
        ignore_scripts: false,
        import_method: PackageImportMethod::Auto,
        logged_methods: &TEST_LOGGED_METHODS,
        rebuild: None,
    }
    .run::<SilentReporter>()
    .expect("build modules must complete cleanly");

    // Drop our writer handle and wait for the task to flush the
    // queued WRITE-path mutation before reading the row back.
    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    let index = StoreIndex::open_readonly_in(&store_dir).expect("open index for read");
    let row = index.get(&files_index_file).expect("get row").expect("row present");
    let side_effects = row.side_effects.expect("side_effects populated");
    let diff = side_effects.get(&expected_cache_key).expect("entry for cache key");
    let added = diff.added.as_ref().expect("added present");
    assert!(
        added.contains_key("generated.txt"),
        "added map should record the postinstall-created file: {added:?}",
    );
    assert!(
        !added.contains_key("index.js"),
        "pristine index.js must NOT appear in `added` (its digest matches base): {added:?}",
    );
}
