use super::{
    super::{BuildModules, allow_build_policy::AllowBuildPolicy, deferred_builds},
    TEST_LOGGED_METHODS, create_buildable_pkg, key, policy_from_specs, root_importers,
};
#[cfg(unix)]
use super::{create_failing_postinstall_fixture, create_postinstall_modifies_source_fixture};
use crate::{RequiresBuildBySnapshot, SkippedSnapshots, VirtualStoreLayout};
use pnpm_config::{Config, PackageImportMethod};
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_lockfile::SnapshotEntry;
use pnpm_reporter::SilentReporter;
#[cfg(unix)]
use pnpm_reporter::{LogEvent, Reporter};
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::sync::Mutex;
use std::{collections::HashMap, fs};
use tempfile::tempdir;

#[test]
fn deferred_builds_uses_only_the_supplied_snapshots() {
    let first = key("first", "1.0.0");
    let second = key("second", "1.0.0");
    let requires_build = HashMap::from([(first.clone(), true), (second, true)]);

    assert_eq!(
        deferred_builds(requires_build.iter().filter(|(key, _)| *key == &first), true),
        [first.to_string()],
    );
}
#[test]
pub(super) fn dangerously_allow_all_builds() {
    let policy = policy_from_specs([], true);
    assert_eq!(policy.check("any-package@1.0.0"), Some(true));
    assert_eq!(policy.check("other-package@2.0.0"), Some(true));
}
/// Wildcards in `allowBuilds` keys are accepted by the parser
/// (they land as literal strings in the expanded set) but the
/// `HashSet::contains` lookup means they never match a real
/// package name. Use `dangerouslyAllowAllBuilds` for blanket
/// allow.
#[test]
fn wildcard_name_in_allow_builds_does_not_match_real_package() {
    let policy = policy_from_specs([("is-*", true)], false);
    assert_eq!(policy.check("is-odd@1.0.0"), None);
    assert_eq!(policy.check("is-positive@1.0.0"), None);
}
#[test]
fn from_config_consumes_allow_builds_and_dangerously_allow_all_builds() {
    let mut config = Config::new();
    config.dangerously_allow_all_builds = false;
    config.allow_builds.insert("@pnpm.e2e/install-script-example".to_string(), true);
    config.allow_builds.insert("@pnpm.e2e/bad-package".to_string(), false);

    let policy = AllowBuildPolicy::from_config(&config).expect("valid specs");
    assert_eq!(policy.check("@pnpm.e2e/install-script-example@1.0.0"), Some(true));
    assert_eq!(policy.check("@pnpm.e2e/bad-package@1.0.0"), Some(false));
    assert_eq!(policy.check("@pnpm.e2e/unrelated@1.0.0"), None);
}
/// Default-deny: a buildable package not listed in `allowBuilds` lands in
/// the returned ignored set, sorted lexically. "Buildable" is computed from
/// the extracted package directory (postinstall in package.json).
#[test]
fn build_modules_collects_ignored_builds() {
    let snapshots = HashMap::from([
        (key("zzz", "1.0.0"), SnapshotEntry::default()),
        (key("aaa", "2.0.0"), SnapshotEntry::default()),
    ]);
    let importers = root_importers(&[("zzz", "1.0.0"), ("aaa", "2.0.0")]);
    let policy = AllowBuildPolicy::default(); // empty → default-deny

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_buildable_pkg(virtual_store_dir.path(), &key("zzz", "1.0.0"));
    create_buildable_pkg(virtual_store_dir.path(), &key("aaa", "2.0.0"));

    let ignored = BuildModules {
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
    .run::<SilentReporter>()
    .expect("run BuildModules")
    .ignored_builds;
    dbg!(&ignored);

    assert_eq!(
        ignored,
        vec!["aaa@2.0.0".to_string(), "zzz@1.0.0".to_string()],
        "ignored set must be sorted lexicographically: {ignored:?}",
    );
}
/// The post-build importer bin relink keys off
/// [`BuildModulesOutput::mutated_slots`], so the signal must stay
/// `false` when every candidate is blocked by the allow policy —
/// nothing wrote into a linked slot, and the relink may skip.
#[test]
fn mutated_slots_is_false_when_every_build_is_ignored() {
    let snapshots = HashMap::from([(key("zzz", "1.0.0"), SnapshotEntry::default())]);
    let importers = root_importers(&[("zzz", "1.0.0")]);
    let policy = AllowBuildPolicy::default(); // empty → default-deny

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");
    create_buildable_pkg(virtual_store_dir.path(), &key("zzz", "1.0.0"));

    let output = BuildModules {
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
    .run::<SilentReporter>()
    .expect("run BuildModules");
    assert!(!output.mutated_slots, "an ignored build must not report a slot mutation");
}
/// The converse of [`mutated_slots_is_false_when_every_build_is_ignored`]:
/// an allowed candidate actually runs its script, so the signal must be
/// `true` and the post-build importer bin relink must not skip.
#[test]
fn mutated_slots_is_true_when_a_script_runs() {
    let snapshots = HashMap::from([(key("zzz", "1.0.0"), SnapshotEntry::default())]);
    let importers = root_importers(&[("zzz", "1.0.0")]);
    let policy = policy_from_specs([("zzz", true)], false);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");
    // `create_buildable_pkg` writes a `postinstall: true` script the
    // policy above allows to run; swap the body for a portable no-op
    // (`true` is not a command on Windows shells).
    let pkg_dir = create_buildable_pkg(virtual_store_dir.path(), &key("zzz", "1.0.0"));
    fs::write(
        pkg_dir.join("package.json"),
        serde_json::json!({ "scripts": { "postinstall": "node -e 0" } }).to_string(),
    )
    .expect("write manifest");

    let output = BuildModules {
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
    .run::<SilentReporter>()
    .expect("run BuildModules");
    assert!(output.mutated_slots, "an executed script must report a slot mutation");
}
/// Under `ignore_scripts`, the same default-deny build candidates that
/// [`build_modules_collects_ignored_builds`] reports as ignored are
/// instead silently skipped: no script runs and the returned set is
/// empty, so the install does not fail with `ERR_PNPM_IGNORED_BUILDS`.
/// With `ignore_scripts` set, the ignored-builds set stays empty.
#[test]
fn ignore_scripts_skips_build_without_collecting_ignored() {
    let snapshots = HashMap::from([
        (key("zzz", "1.0.0"), SnapshotEntry::default()),
        (key("aaa", "2.0.0"), SnapshotEntry::default()),
    ]);
    let importers = root_importers(&[("zzz", "1.0.0"), ("aaa", "2.0.0")]);
    let policy = AllowBuildPolicy::default(); // empty → default-deny

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_buildable_pkg(virtual_store_dir.path(), &key("zzz", "1.0.0"));
    create_buildable_pkg(virtual_store_dir.path(), &key("aaa", "2.0.0"));

    let ignored = BuildModules {
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
        ignore_scripts: true,
        import_method: PackageImportMethod::Auto,
        logged_methods: &TEST_LOGGED_METHODS,
        rebuild: None,
    }
    .run::<SilentReporter>()
    .expect("run BuildModules")
    .ignored_builds;
    dbg!(&ignored);

    assert!(ignored.is_empty(), "ignore_scripts must not collect ignored builds: {ignored:?}");
}
#[test]
fn cached_requires_build_false_skips_package_dir_probe() {
    let pkg_key = key("aaa", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let importers = root_importers(&[("aaa", "1.0.0")]);
    let policy = AllowBuildPolicy::default();

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_buildable_pkg(virtual_store_dir.path(), &pkg_key);
    let requires_build_by_snapshot = RequiresBuildBySnapshot::from([(pkg_key, false)]);

    let ignored = BuildModules {
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
        requires_build_by_snapshot: Some(&requires_build_by_snapshot),
        engine_name: None,
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
    .run::<SilentReporter>()
    .expect("run BuildModules")
    .ignored_builds;

    assert!(ignored.is_empty());
}
/// Parallel-path variant of [`build_modules_collects_ignored_builds`]
/// running under `child_concurrency: 2`. The other `BuildModules`
/// tests all run with `child_concurrency: 1`; this test pins the
/// concurrent codepath against two independent policy-denied build
/// candidates.
///
/// The assertion is the same sorted ignored-set as the sequential
/// test. The concurrently scheduled nodes may insert in a
/// non-deterministic order. The
/// `BTreeSet`-backed `ignored_builds` ordering hides that, so
/// breakage would more likely show up as a lock contention bug
/// (e.g. dropping the `Mutex` wrapping) which would manifest as a
/// rustc / clippy error rather than a runtime failure; this test
/// at least documents that the codepath is exercised.
#[test]
fn build_modules_collects_ignored_builds_under_concurrency() {
    let snapshots = HashMap::from([
        (key("zzz", "1.0.0"), SnapshotEntry::default()),
        (key("aaa", "2.0.0"), SnapshotEntry::default()),
    ]);
    let importers = root_importers(&[("zzz", "1.0.0"), ("aaa", "2.0.0")]);
    let policy = AllowBuildPolicy::default();

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_buildable_pkg(virtual_store_dir.path(), &key("zzz", "1.0.0"));
    create_buildable_pkg(virtual_store_dir.path(), &key("aaa", "2.0.0"));

    let ignored = BuildModules {
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
        child_concurrency: 2,
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
    .expect("run BuildModules under concurrency")
    .ignored_builds;
    dbg!(&ignored);

    // Same expected output as the sequential test. Both independent
    // nodes may insert concurrently, and the BTreeSet's iteration
    // order normalizes the result.
    assert_eq!(
        ignored,
        vec!["aaa@2.0.0".to_string(), "zzz@1.0.0".to_string()],
        "ignored set must be the same under concurrency 2 as under 1: {ignored:?}",
    );
}
/// Explicit `false` in `allowBuilds` is silently skipped — it does NOT
/// land in the ignored-scripts list.
#[test]
fn build_modules_excludes_explicit_deny_from_ignored() {
    let snapshots = HashMap::from([
        (key("denied", "1.0.0"), SnapshotEntry::default()),
        (key("ignored", "1.0.0"), SnapshotEntry::default()),
    ]);
    let importers = root_importers(&[("denied", "1.0.0"), ("ignored", "1.0.0")]);

    let policy = policy_from_specs([("denied", false)], false);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_buildable_pkg(virtual_store_dir.path(), &key("denied", "1.0.0"));
    create_buildable_pkg(virtual_store_dir.path(), &key("ignored", "1.0.0"));

    let ignored = BuildModules {
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
    .run::<SilentReporter>()
    .expect("run BuildModules")
    .ignored_builds;
    dbg!(&ignored);

    assert_eq!(
        ignored,
        vec!["ignored@1.0.0".to_string()],
        "explicit-false must NOT appear in ignored set: {ignored:?}",
    );
}
/// A side-effects cache hit skips the rebuild.
///
/// The cache is normally populated by running the install twice —
/// once via the WRITE path, then consumed. Pacquet doesn't have a
/// WRITE path yet ([#421]'s slice (B)), so we hand-craft the same
/// state directly: a `side_effects_maps_by_snapshot` entry whose
/// cache key matches what `BuildModules` will compute via
/// `calc_dep_state`. With that in place, the gate skips the build
/// even though the package's `postinstall` would have failed —
/// observable via the absence of a `pnpm:lifecycle` event for the
/// stage.
///
/// The fixture (`@pnpm.e2e/failing-postinstall@1.0.0`, `postinstall:
/// echo hello && echo world && exit 1`); if the gate were broken the
/// build would run and the install would propagate the exit-1
/// failure (cf. [`fail_when_failing_postinstall_is_required`] below).
///
/// [#421]: https://github.com/pnpm/pacquet/issues/421
#[cfg(unix)]
#[test]
fn using_side_effects_cache_skips_rebuild() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("lock").push(event.clone());
        }
    }

    let pkg_key = key("@pnpm.e2e/failing-postinstall", "1.0.0");
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
    let importers = root_importers(&[("@pnpm.e2e/failing-postinstall", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_failing_postinstall_fixture(virtual_store_dir.path(), &pkg_key);

    // Compute the cache key the same way `BuildModules` will, then
    // pre-populate `side_effects_maps_by_snapshot` with a matching
    // entry. The overlay's FilesMap is the post-build file set
    // (resolved to CAS paths); the gate re-materializes it into the
    // already-linked slot, so it carries both the pristine
    // `package.json` and a side-effect file the build would have
    // produced — proving the cached build output lands on disk
    // without the script re-running.
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
    let pkg_dir = virtual_store_dir
        .path()
        .join("@pnpm.e2e+failing-postinstall@1.0.0")
        .join("node_modules")
        .join("@pnpm.e2e/failing-postinstall");
    let cas_source = tempdir().expect("create temp dir");
    let side_effect_blob = cas_source.path().join("generated-by-postinstall");
    fs::write(&side_effect_blob, b"built").expect("write side-effect blob");
    let mut overlay = std::collections::HashMap::new();
    overlay.insert(
        expected_cache_key,
        std::collections::HashMap::from([
            ("package.json".to_string(), pkg_dir.join("package.json")),
            ("generated-by-postinstall.js".to_string(), side_effect_blob),
        ]),
    );
    let mut side_effects_maps = std::collections::HashMap::new();
    side_effects_maps.insert(pkg_key.clone(), std::sync::Arc::new(overlay));

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
    .run::<RecordingReporter>()
    .expect("install must succeed when the cache hit skips the rebuild");

    // The build was skipped, so no `pnpm:lifecycle` event for the
    // postinstall stage should have been emitted. If the gate were
    // broken the failing-postinstall script would have run and
    // emitted a `Script` (and a non-zero `Exit`) event, plus
    // returned `Err(BuildModulesError::LifecycleScript(...))` from
    // `.run()`.
    let captured = EVENTS.lock().expect("lock").clone();
    let any_lifecycle = captured.iter().any(|e| matches!(e, LogEvent::Lifecycle(_)));
    assert!(!any_lifecycle, "side-effects cache hit must skip lifecycle scripts: {captured:#?}");

    // The script was skipped, but the cached build output still has to
    // be materialized — the overlay's side-effect file must land in the
    // slot so the warm reinstall isn't left in its pre-build state.
    assert!(
        pkg_dir.join("generated-by-postinstall.js").exists(),
        "cached side-effect file must be materialized when the gate skips the rebuild",
    );
}
/// A cache hit whose overlay can't be materialized (e.g. a side-effects
/// `added` blob deleted out from under the store — those aren't
/// re-verified) must not abort the install. It degrades to a cache miss:
/// the build re-runs over the pristine files and re-seeds the cache,
/// instead of propagating the import error past the optional-dependency
/// swallow below.
#[cfg(unix)]
#[test]
fn corrupt_side_effects_cache_falls_back_to_rebuild() {
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

    let (pkg_dir, _mode) =
        create_postinstall_modifies_source_fixture(virtual_store_dir.path(), &pkg_key);

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
    // The overlay points `generated.txt` at a CAS path that doesn't
    // exist, so `materialize_side_effects` fails — standing in for a
    // store whose side-effects blob went missing.
    let overlay = std::collections::HashMap::from([
        ("package.json".to_string(), pkg_dir.join("package.json")),
        ("generated.txt".to_string(), virtual_store_dir.path().join("missing-cas-blob")),
    ]);
    let mut side_effects_maps = std::collections::HashMap::new();
    side_effects_maps.insert(
        pkg_key.clone(),
        std::sync::Arc::new(HashMap::from([(expected_cache_key, overlay)])),
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
        side_effects_maps_by_snapshot: Some(&side_effects_maps),
        requires_build_by_snapshot: None,
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
    .run::<SilentReporter>()
    .expect("a corrupt cache overlay must degrade to a rebuild, not abort the install");

    assert!(
        pkg_dir.join("generated.txt").exists(),
        "rebuild must run when the cached overlay can't be materialized",
    );
}
