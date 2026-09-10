#[cfg(unix)]
use super::{
    super::BuildModules, TEST_LOGGED_METHODS, create_failing_postinstall_fixture,
    create_postinstall_modifies_source_fixture, create_postinstall_with_unreadable_fixture, key,
    root_importers,
};
use super::{
    super::{
        allow_build_policy::AllowBuildPolicy,
        slots::{is_contained_descendant, parse_name_version_from_key},
    },
    policy_from_specs,
};
#[cfg(unix)]
use crate::SkippedSnapshots;
use crate::VirtualStoreLayout;
use pnpm_config::Config;
#[cfg(unix)]
use pnpm_config::PackageImportMethod;
#[cfg(unix)]
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_lockfile::PackageKey;
#[cfg(unix)]
use pnpm_lockfile::SnapshotEntry;
#[cfg(unix)]
use pnpm_reporter::{
    LogEvent, Reporter, SilentReporter, SkippedOptionalPackage, SkippedOptionalReason,
};
use pretty_assertions::assert_eq;
use std::path::Path;
#[cfg(unix)]
use std::{collections::HashMap, fs, sync::Mutex};
use tempfile::tempdir;

#[test]
fn parse_key_without_leading_slash() {
    let (name, version) = parse_name_version_from_key("express@4.18.1");
    assert_eq!(name, "express");
    assert_eq!(version, "4.18.1");
}
#[test]
fn explicit_allow() {
    let policy = policy_from_specs([("@pnpm.e2e/install-script-example", true)], false);
    assert_eq!(policy.check("@pnpm.e2e/install-script-example@1.0.0"), Some(true));
}
#[test]
fn explicit_allow_requires_registry_style_dep_path() {
    let policy = policy_from_specs([("@pnpm.e2e/install-script-example", true)], false);
    assert_eq!(
        policy.check("@pnpm.e2e/install-script-example@git+https://example.com/x.git#abc123"),
        None,
    );
    assert_eq!(policy.check("@pnpm.e2e/install-script-example@1.0.0"), Some(true));
}
#[test]
fn explicit_allow_by_git_hosted_tarball_repo_url() {
    let policy = policy_from_specs(
        [
            ("foo@git+https://github.com/org/foo.git", true),
            ("bar@git+https://bitbucket.org/org/bar.git", true),
            ("baz@git+https://gitlab.com/group/subgroup/baz.git", true),
            ("evil@git+https://github.com/org/evil.git", false),
            // Slash-bearing keys the buggy multi-segment parse would have produced.
            ("qux@git+https://github.com/org/extra/qux.git", true),
            ("quux@git+https://bitbucket.org/org/extra/quux.git", true),
        ],
        false,
    );

    // A GitHub `github:` dependency is downloaded from codeload.github.com, yet
    // the same key a clone of the repo would use approves it — no commit hash.
    assert_eq!(policy.check("foo@https://codeload.github.com/org/foo/tar.gz/abc123"), Some(true));
    assert_eq!(
        policy.check("foo@https://codeload.github.com/org/foo/tar.gz/def456(react@19.0.0)"),
        Some(true),
    );
    // Bitbucket and GitLab (with nested groups) tarball downloads too.
    assert_eq!(policy.check("bar@https://bitbucket.org/org/bar/get/abc123.tar.gz"), Some(true));
    assert_eq!(
        policy
            .check("baz@https://gitlab.com/group/subgroup/baz/-/archive/abc123/baz-abc123.tar.gz"),
        Some(true),
    );
    // A different repository under the same package name is not approved.
    assert_eq!(policy.check("foo@https://codeload.github.com/attacker/foo/tar.gz/abc123"), None);
    // A look-alike download host must not be rewritten into the trusted key.
    assert_eq!(
        policy.check("foo@https://codeload.github.com.attacker.net/org/foo/tar.gz/abc123"),
        None,
    );
    // Denial by hashless repository key works as well.
    assert_eq!(
        policy.check("evil@https://codeload.github.com/org/evil/tar.gz/abc123"),
        Some(false),
    );

    // A tarball URL with an extra path segment is not a valid codeload/get URL
    // (a repo is exactly `owner/repo`) and must not be normalized, matching the
    // `[^/]+` repo anchor in the TypeScript matcher. Even with the slash-bearing
    // key allowlisted, the multi-segment URL stays unapproved.
    assert_eq!(policy.check("qux@https://codeload.github.com/org/extra/qux/tar.gz/abc123"), None);
    assert_eq!(policy.check("quux@https://bitbucket.org/org/extra/quux/get/abc123.tar.gz"), None);
    // A URL on a claimed download host that does not match that host's tarball
    // pattern is rejected outright — it must not fall through to the generic
    // GitLab matcher and produce the host's trusted repo key.
    assert_eq!(
        policy.check("bar@https://bitbucket.org/org/bar/-/archive/abc123/bar-abc123.tar.gz"),
        None,
    );
    assert_eq!(
        policy.check("foo@https://codeload.github.com/org/foo/-/archive/abc123/foo-abc123.tar.gz"),
        None,
    );
    // A GitLab archive URL without a `<ref>/` segment after the marker is not
    // normalized.
    assert_eq!(policy.check("baz@https://gitlab.com/group/subgroup/baz/-/archive/abc123"), None);
}
#[test]
fn explicit_deny() {
    let policy = policy_from_specs([("@pnpm.e2e/bad-package", false)], false);
    assert_eq!(policy.check("@pnpm.e2e/bad-package@1.0.0"), Some(false));
}
#[test]
fn unlisted_returns_none() {
    let policy = policy_from_specs([("@pnpm.e2e/allowed", true)], false);
    assert_eq!(policy.check("@pnpm.e2e/not-listed@1.0.0"), None);
}
/// The disallowed set is checked before the allowed set, so a
/// bare-name disallow wins over an exact-version allow.
#[test]
fn disallow_bare_name_wins_over_allow_exact_version() {
    let policy =
        policy_from_specs([("@pnpm.e2e/pkg@1.0.0", true), ("@pnpm.e2e/pkg", false)], false);
    assert_eq!(policy.check("@pnpm.e2e/pkg@1.0.0"), Some(false));
    assert_eq!(policy.check("@pnpm.e2e/pkg@2.0.0"), Some(false));
}
/// The converse: a bare-name allow combined with an exact-version
/// disallow → the disallow on `pkg@1.0.0` fires only for that
/// version; other versions hit the bare-name allow.
#[test]
fn disallow_exact_version_with_allow_bare_name() {
    let policy =
        policy_from_specs([("@pnpm.e2e/pkg", true), ("@pnpm.e2e/pkg@1.0.0", false)], false);
    assert_eq!(policy.check("@pnpm.e2e/pkg@1.0.0"), Some(false));
    assert_eq!(policy.check("@pnpm.e2e/pkg@2.0.0"), Some(true));
}
#[test]
fn empty_rules_denies_all() {
    let policy = policy_from_specs([], false);
    assert_eq!(policy.check("any-package@1.0.0"), None);
}
#[test]
fn dangerously_allow_all_allows_artifact_dep_paths() {
    let policy = policy_from_specs([], true);
    assert_eq!(policy.check("anything@git+https://example.com/x.git#abc123"), Some(true));
}
/// Version unions expand into separate exact-version allows.
/// `qar@1.0.0 || 2.0.0` allows exactly those two versions, leaves
/// other versions unlisted (`None`).
#[test]
fn allow_via_version_union() {
    let policy = policy_from_specs([("foo", true), ("qar@1.0.0 || 2.0.0", true)], false);
    assert_eq!(policy.check("foo@1.0.0"), Some(true));
    assert_eq!(policy.check("bar@1.0.0"), None);
    assert_eq!(policy.check("qar@1.0.0"), Some(true));
    assert_eq!(policy.check("qar@2.0.0"), Some(true));
    assert_eq!(policy.check("qar@1.1.0"), None);
}
/// `from_config` propagates `expand_package_version_specs` errors —
/// an invalid version union in `Config.allow_builds` surfaces as
/// `ERR_PNPM_INVALID_VERSION_UNION` rather than silently dropping
/// the rule.
#[test]
fn from_config_propagates_invalid_version_union() {
    let mut config = Config::new();
    config.allow_builds.insert("foo@not-a-version".to_string(), true);
    let err = AllowBuildPolicy::from_config(&config).expect_err("must reject");
    assert!(matches!(err, crate::VersionPolicyError::InvalidVersionUnion { .. }), "got: {err:?}");
}
#[test]
fn from_config_propagates_name_pattern_in_version_union() {
    let mut config = Config::new();
    config.allow_builds.insert("foo*@1.0.0".to_string(), true);
    let err = AllowBuildPolicy::from_config(&config).expect_err("must reject");
    assert!(
        matches!(err, crate::VersionPolicyError::NamePatternInVersionUnion { .. }),
        "got: {err:?}",
    );
}
#[test]
fn empty_config_denies_all() {
    let policy = AllowBuildPolicy::from_config(&Config::new()).expect("empty config never errors");
    assert_eq!(policy.check("anything@1.0.0"), None);
}
/// Optional dep whose postinstall fails must be reported through the
/// `pnpm:skipped-optional-dependency` channel (reason `build_failure`)
/// and NOT abort the install.
///
/// The test uses the `@pnpm.e2e/failing-postinstall@1.0.0` fixture
/// (`postinstall: echo hello && echo world && exit 1`).
///
/// Unix-gated because the script is POSIX shell syntax. The
/// cmd-on-Windows path picks a different shell —
/// `pnpm_executor::select_shell` (tested in the executor crate's
/// `shell::tests`) covers the shell-selection branches in isolation.
#[cfg(unix)]
#[test]
fn do_not_fail_on_optional_dep_with_failing_postinstall() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().expect("lock").clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().expect("lock").push(event.clone());
        }
    }

    let pkg_key = key("@pnpm.e2e/failing-postinstall", "1.0.0");
    let optional_snapshot = SnapshotEntry { optional: true, ..Default::default() };
    let snapshots = HashMap::from([(pkg_key.clone(), optional_snapshot)]);
    let importers = root_importers(&[("@pnpm.e2e/failing-postinstall", "1.0.0")]);
    // `dangerouslyAllowAllBuilds` so the policy lets the failing
    // script through to actually run — this test exercises the
    // build-failure path, not the policy gate.
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_failing_postinstall_fixture(virtual_store_dir.path(), &pkg_key);

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
    .run::<RecordingReporter>()
    .expect("optional build failure must NOT abort the install");
    dbg!(&ignored);

    let captured = EVENTS.lock().expect("lock").clone();
    dbg!(&captured);
    let skipped_event = captured
        .iter()
        .find_map(|event| match event {
            LogEvent::SkippedOptionalDependency(log) => Some(log),
            _ => None,
        })
        .expect("must emit pnpm:skipped-optional-dependency");
    assert_eq!(skipped_event.reason, SkippedOptionalReason::BuildFailure);
    let SkippedOptionalPackage::Installed { name, version, .. } = &skipped_event.package else {
        panic!("expected Installed payload for build_failure, got {:?}", skipped_event.package);
    };
    assert_eq!(name, "@pnpm.e2e/failing-postinstall");
    assert_eq!(version, "1.0.0");
    assert!(skipped_event.details.is_some(), "details must carry the error toString");
}
/// A package that is both an optional and a non-optional dependency
/// fails the install when its postinstall fails.
///
/// The resolver folds reachability ALL-paths-optional, so a package
/// reachable through any non-optional edge has
/// `snapshots[...].optional = false` in the lockfile. `BuildModules`
/// then propagates the build failure rather than swallowing it.
/// Pacquet trusts the precomputed flag; this test pins the
/// propagation branch by supplying the fixture with `optional:
/// false`, the lockfile shape produced for the dual-reachability
/// case.
#[cfg(unix)]
#[test]
pub(super) fn fail_when_failing_postinstall_is_required() {
    let pkg_key = key("@pnpm.e2e/failing-postinstall", "1.0.0");
    // `optional: false` — the ALL-paths-optional fold concluded the
    // dep is required.
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let importers = root_importers(&[("@pnpm.e2e/failing-postinstall", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    create_failing_postinstall_fixture(virtual_store_dir.path(), &pkg_key);

    let err = BuildModules {
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
    .expect_err("required build failure must propagate");
    eprintln!("ERR: {err}");
    assert!(matches!(err, crate::build_modules::BuildModulesError::LifecycleScript(_)));
}
/// Counterpart of the WRITE-path test: with `side_effects_cache_write
/// = false`, the same fixture's row must come out of `BuildModules`
/// with `side_effects = None`.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn write_path_disabled_skips_upload() {
    use pnpm_store_dir::{
        HASH_ALGORITHM, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexWriter, store_index_key,
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

    let (_pkg_dir, _actual_mode) =
        create_postinstall_modifies_source_fixture(virtual_store_dir.path(), &pkg_key);

    let files_index_file = store_index_key(integrity_str, &pkg_key.without_peer().to_string());
    let base_row = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        requires_prepare: None,
        algo: HASH_ALGORITHM.to_string(),
        files: HashMap::new(),
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
        engine_name: Some("darwin;arm64;node20"),
        side_effects_cache: true,
        side_effects_cache_write: false,
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

    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    let index = StoreIndex::open_readonly_in(&store_dir).expect("open index for read");
    let row = index.get(&files_index_file).expect("get row").expect("row present");
    assert!(row.side_effects.is_none(), "write disabled must NOT populate side_effects");
}
/// Uploading errors do not interrupt the install: the install
/// completes (the postinstall ran, the generated file is on disk)
/// but the `SQLite` row's `side_effects` stays empty.
///
/// Pacquet has no DI seam for the upload, but the WRITE path's
/// only failure point is `add_files_from_dir`, which surfaces as
/// `UploadError::AddFilesFromDir`. We force that failure by having
/// the postinstall script create a 0-permission file in the
/// package directory: `add_files_from_dir` then fails to `fs::read`
/// it, returning an error that `BuildModules` swallows with
/// `tracing::warn!`. The install completes, the postinstall-generated
/// artifact is on disk, and the build keeps going.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn upload_error_does_not_interrupt_install() {
    use pnpm_store_dir::{
        HASH_ALGORITHM, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexWriter, store_index_key,
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

    // Fixture variant: postinstall produces a regular file (to
    // prove the script ran end-to-end) AND a 0-permission file
    // (to force `add_files_from_dir` to fail on `fs::read`).
    let pkg_dir = create_postinstall_with_unreadable_fixture(virtual_store_dir.path(), &pkg_key);

    // Pre-seed a base row so we can assert that the swallowed
    // upload error leaves the row's `side_effects` field untouched.
    let files_index_file = store_index_key(integrity_str, &pkg_key.without_peer().to_string());
    let base_row = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        requires_prepare: None,
        algo: HASH_ALGORITHM.to_string(),
        files: HashMap::new(),
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
        frozen_store: false,
        ignore_scripts: false,
        import_method: PackageImportMethod::Auto,
        logged_methods: &TEST_LOGGED_METHODS,
        rebuild: None,
    }
    .run::<SilentReporter>()
    .expect("upload failure must not propagate; install continues");

    drop(writer);
    writer_task.await.expect("await writer").expect("writer succeeds");

    assert!(
        pkg_dir.join("generated.txt").exists(),
        "postinstall-created file must be present after a swallowed upload failure",
    );

    // The base row stays untouched: the `add_files_from_dir`
    // error fired before `queue_side_effects_upload` ran, so the
    // writer task never saw a `SideEffectsUpload` for this row.
    let index = StoreIndex::open_readonly_in(&store_dir).expect("open index for read");
    let row = index.get(&files_index_file).expect("get row").expect("base row present");
    assert!(
        row.side_effects.is_none(),
        "swallowed upload error must leave `side_effects` unmodified",
    );

    // Restore perms so the tempdir cleanup can remove the file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(pkg_dir.join("unreadable"), fs::Permissions::from_mode(0o644));
    }
}
// --- pkg_root_for_key ---------------------------------------------------

/// Without an override map (the isolated linker), the helper falls
/// through to `virtual_store_dir_for_key` and returns
/// `Some(<slot>/node_modules/<name>)`. Pinning the `Some` shape
/// (rather than just the path) catches a future change that turns
/// the isolated path optional too.
#[test]
fn pkg_root_for_key_isolated_uses_layout() {
    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.virtual_store_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();
    let layout = VirtualStoreLayout::new(config, None, None, None, None, None);

    let key: PackageKey = "is-positive@1.0.0".parse().expect("parse key");
    let result = super::super::PkgRoots { layout: &layout, by_key: None }
        .canonical(&key)
        .expect("isolated lookup hits");

    assert!(
        result.starts_with(&config.virtual_store_dir),
        "isolated pkg_dir lives under the virtual store: {result:?}",
    );
    assert!(
        result.ends_with("node_modules/is-positive"),
        "trailing path is `node_modules/<name>`: {result:?}",
    );
}
/// The GVS build-failure cleanup only recurse-deletes a slot that sits
/// strictly inside the store root through `..`-free components, so a
/// crafted package name cannot turn the cleanup into a path traversal.
#[test]
fn is_contained_descendant_rejects_traversal_and_escapes() {
    let root = Path::new("/store/v11/links");

    // A normal GVS slot suffix is accepted.
    assert!(is_contained_descendant(root, &root.join("@pnpm.e2e/foo/1.0.0/deadbeef")));
    assert!(is_contained_descendant(root, &root.join("foo/1.0.0/deadbeef")));

    // A `..` segment that climbs out of the root is rejected even though
    // the path still textually starts with the root.
    assert!(!is_contained_descendant(root, &root.join("../../../etc/passwd")));
    assert!(!is_contained_descendant(root, &root.join("foo/../../../escape")));

    // The root itself is not a descendant — deleting it wholesale is not
    // a per-slot cleanup.
    assert!(!is_contained_descendant(root, root));

    // A sibling that merely shares a name prefix is not contained.
    assert!(!is_contained_descendant(root, Path::new("/store/v11/links-evil/foo")));
}
