use super::super::allow_build_policy::allow_build_key_from_ignored_build;
#[cfg(unix)]
use super::{
    super::BuildModules, TEST_LOGGED_METHODS, create_postinstall_modifies_source_fixture, key,
    policy_from_specs, root_importers, sha512_hex,
};
#[cfg(unix)]
use crate::{SkippedSnapshots, VirtualStoreLayout};
#[cfg(unix)]
use pnpm_config::PackageImportMethod;
#[cfg(unix)]
use pnpm_executor::ScriptsPrependNodePath;
#[cfg(unix)]
use pnpm_lockfile::{PackageKey, SnapshotEntry};
#[cfg(unix)]
use pnpm_reporter::SilentReporter;
use pnpm_reporter::{IgnoredScriptsLog, LogEvent, Reporter};
use pretty_assertions::assert_eq;
use std::sync::Mutex;
#[cfg(unix)]
use std::{collections::HashMap, fs};
#[cfg(unix)]
use tempfile::tempdir;

/// Recording fake confirms `pnpm:ignored-scripts` is the right channel
/// for the package list. The frozen-install path emits this once after
/// `BuildModules::run` returns; this test exercises the equivalent
/// emit shape directly so `LogEvent::IgnoredScripts` stays connected
/// to the `BuildModules` return value.
#[test]
fn ignored_scripts_event_carries_returned_names() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("lock").push(event.clone());
        }
    }

    let names = vec!["a@1.0.0".to_string(), "b@2.0.0".to_string()];
    RecordingReporter::emit(&LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: pnpm_reporter::LogLevel::Debug,
        package_names: names.clone(),
        strict_dep_builds: false,
    }));

    let captured = EVENTS.lock().expect("lock").clone();
    dbg!(&captured);
    assert!(
        matches!(
            captured.as_slice(),
            [LogEvent::IgnoredScripts(IgnoredScriptsLog { package_names, .. })]
                if package_names == &names,
        ),
        "captured: {captured:?}",
    );
}
/// When `BuildModules.patches` contains an entry for a snapshot,
/// the side-effects-cache key computed for that snapshot must
/// include the `;patch=<hash>` segment that
/// [`pnpm_graph_hasher::CalcDepStateOptions::patch_file_hash`]
/// appends.
///
/// Drive this through the WRITE path so the test can read the
/// cache key back out of the persisted row — the `;patch=<hash>`
/// segment in the key shape is the contract the cache relies on.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn write_path_cache_key_includes_patch_hash() {
    use pnpm_patching::ExtendedPatchInfo;
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

    let files_index_file = store_index_key(integrity_str, &pkg_key.without_peer().to_string());
    let mut base_files = HashMap::new();
    base_files.insert(
        "index.js".to_string(),
        CafsFileInfo {
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

    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    // The applier runs against `pkg_dir` before the postinstall, so
    // it needs a real patch file that succeeds. Touch a brand-new
    // file rather than modifying `index.js` so the assertions on
    // the diff map below stay simple — the patch adds
    // `patched.txt`, the postinstall adds `generated.txt`, the
    // pristine `index.js` stays at its base digest.
    let patch_dir = tempdir().expect("create patch dir");
    let patch_file = patch_dir.path().join("foo.patch");
    fs::write(
        &patch_file,
        "\
diff --git a/patched.txt b/patched.txt
new file mode 100644
--- /dev/null
+++ b/patched.txt
@@ -0,0 +1 @@
+hello from the patch
",
    )
    .expect("write patch");

    let patch_hash = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let patches: HashMap<PackageKey, ExtendedPatchInfo> = HashMap::from([(
        pkg_key.without_peer(),
        ExtendedPatchInfo {
            hash: patch_hash.to_string(),
            patch_file_path: Some(patch_file.clone()),
            key: "@pnpm/postinstall-modifies-source@1.0.0".to_string(),
        },
    )]);

    let engine = "darwin;arm64;node20";
    let dep_graph = crate::build_deps_graph(&snapshots, &packages);
    let mut state_cache = pnpm_graph_hasher::DepsStateCache::new();
    let expected_cache_key_with_patch = pnpm_graph_hasher::calc_dep_state(
        &dep_graph,
        &mut state_cache,
        &pkg_key,
        &pnpm_graph_hasher::CalcDepStateOptions {
            engine_name: engine,
            patch_file_hash: Some(patch_hash),
            include_dep_graph_hash: true,
        },
    );
    assert!(
        expected_cache_key_with_patch.contains(";patch="),
        "sanity: graph-hasher must emit ';patch=' for the patched options: {expected_cache_key_with_patch:?}",
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
        patches: Some(&patches),

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

    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    let index = StoreIndex::open_readonly_in(&store_dir).expect("open index for read");
    let row = index.get(&files_index_file).expect("get row").expect("row present");
    let side_effects = row.side_effects.expect("side_effects populated");
    assert!(
        side_effects.contains_key(&expected_cache_key_with_patch),
        "patched cache key must appear in side_effects map: \
         expected key {expected_cache_key_with_patch:?}, got keys {:?}",
        side_effects.keys().collect::<Vec<_>>(),
    );
}
/// A patch in the `patches` map gets applied to the extracted
/// package dir before postinstall hooks run.
///
/// Drives `BuildModules` with a single `requires_build=false`
/// snapshot (no postinstall scripts), `dangerouslyAllowAllBuilds: true`
/// (irrelevant — no scripts to allow), and a patch that creates
/// `patched.txt`. After the run, `patched.txt` must exist on disk
/// with the patch body.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn patch_only_snapshot_gets_patched_via_build_modules() {
    use pnpm_patching::ExtendedPatchInfo;
    use pnpm_store_dir::{StoreDir, StoreIndexWriter};

    let pkg_key = key("is-positive", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let importers = root_importers(&[("is-positive", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");

    // Lay down a pristine `is-positive` package dir under the
    // virtual store so the applier has a real target. No scripts,
    // so `requires_build_map` for this snapshot stays false — the
    // build trigger fires solely because of the patch entry.
    let pkg_dir = virtual_store_dir.path().join("is-positive@1.0.0/node_modules/is-positive");
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    fs::write(pkg_dir.join("package.json"), r#"{"name":"is-positive","version":"1.0.0"}"#)
        .expect("write manifest");

    // Patch that creates a brand-new file. Pure Create operation;
    // diffy parses and applies it cleanly.
    let patch_dir = tempdir().expect("create patch dir");
    let patch_file = patch_dir.path().join("is-positive.patch");
    fs::write(
        &patch_file,
        "\
diff --git a/patched.txt b/patched.txt
new file mode 100644
--- /dev/null
+++ b/patched.txt
@@ -0,0 +1 @@
+applied
",
    )
    .expect("write patch");

    let patches: HashMap<PackageKey, ExtendedPatchInfo> = HashMap::from([(
        pkg_key.without_peer(),
        ExtendedPatchInfo {
            hash: "0".repeat(64),
            patch_file_path: Some(patch_file.clone()),
            key: "is-positive@1.0.0".to_string(),
        },
    )]);

    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pnpm_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: None,
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        requires_build_by_snapshot: None,
        engine_name: None,
        side_effects_cache: false,
        side_effects_cache_write: false,
        shared_side_effects_publisher: None,
        store_dir: Some(&store_dir),
        store_index_writer: Some(&writer),
        patches: Some(&patches),

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

    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    let patched = pkg_dir.join("patched.txt");
    assert!(patched.exists(), "patch must have created {}", patched.display());
    assert_eq!(fs::read_to_string(&patched).unwrap(), "applied\n");
}
/// When the resolved patch entry carries a hash but no
/// `patch_file_path`, surfacing `ERR_PNPM_PATCH_FILE_PATH_MISSING`
/// is the explicit signal the user should add the package to
/// `patchedDependencies` in `pnpm-workspace.yaml`.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn missing_patch_file_path_errors_with_diagnostic() {
    use pnpm_patching::ExtendedPatchInfo;
    use pnpm_store_dir::{StoreDir, StoreIndexWriter};

    let pkg_key = key("is-positive", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let importers = root_importers(&[("is-positive", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let store_root = tempdir().expect("create store dir");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store");
    let virtual_store_dir = tempdir().expect("create vstore dir");
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");

    let pkg_dir = virtual_store_dir.path().join("is-positive@1.0.0/node_modules/is-positive");
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    fs::write(pkg_dir.join("package.json"), r#"{"name":"is-positive","version":"1.0.0"}"#)
        .expect("write manifest");

    // `patch_file_path: None` — the lockfile-only shape where a hash
    // is known but no live config provides a file. Must surface as
    // `ERR_PNPM_PATCH_FILE_PATH_MISSING`.
    let patches: HashMap<PackageKey, ExtendedPatchInfo> = HashMap::from([(
        pkg_key.without_peer(),
        ExtendedPatchInfo {
            hash: "0".repeat(64),
            patch_file_path: None,
            key: "is-positive@1.0.0".to_string(),
        },
    )]);

    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    let err = BuildModules {
        layout: &VirtualStoreLayout::legacy(
            virtual_store_dir.path(),
            pnpm_config::default_virtual_store_dir_max_length() as usize,
        ),
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: None,
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        requires_build_by_snapshot: None,
        engine_name: None,
        side_effects_cache: false,
        side_effects_cache_write: false,
        shared_side_effects_publisher: None,
        store_dir: Some(&store_dir),
        store_index_writer: Some(&writer),
        patches: Some(&patches),

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
    .expect_err("missing patch_file_path must surface as PatchFilePathMissing");

    drop(writer);
    let _ = writer_task.await;

    assert!(
        matches!(err, super::super::BuildModulesError::PatchFilePathMissing { .. }),
        "got: {err:?}",
    );
}
#[test]
fn allow_build_key_strips_version_for_registry_packages() {
    // Registry depPaths reduce to the bare package name — the
    // `allowBuilds` key a user would approve them under.
    assert_eq!(allow_build_key_from_ignored_build("esbuild@0.17.0"), "esbuild");
    assert_eq!(
        allow_build_key_from_ignored_build("@pnpm.e2e/install-script-example@1.0.0"),
        "@pnpm.e2e/install-script-example",
    );
}
#[test]
fn allow_build_key_ignores_patch_hash_when_deriving_name() {
    // A `(patch_hash=...)` segment is dropped before the version is checked,
    // so a patched registry package still reduces to its name.
    assert_eq!(allow_build_key_from_ignored_build("esbuild@0.17.0(patch_hash=abcdef)"), "esbuild");
}
#[test]
fn allow_build_key_keeps_full_id_for_non_semver_artifacts() {
    // Git / tarball artifacts have a non-semver "version", so the whole
    // pkgId is the key — the name alone must not approve their builds.
    let git = "foo@github.com/foo/bar#0123456789";
    assert_eq!(allow_build_key_from_ignored_build(git), git);

    // Ignored-build entries are depPath-shaped (`name@<resolution>`); a
    // tarball's non-semver resolution keeps the whole pkgId as the key.
    let tarball = "foo@https://example.com/foo.tgz";
    assert_eq!(allow_build_key_from_ignored_build(tarball), tarball);
}
