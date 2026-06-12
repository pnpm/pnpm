use super::{
    Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install, current_settings,
};
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_modules_yaml::IncludedDependencies;
use pacquet_package_manifest::PackageManifest;
use pacquet_workspace_state::{
    ProjectEntry, WorkspaceState, WorkspaceStateSettings, now_millis, update_workspace_state,
};
use std::{collections::BTreeMap, fs, thread::sleep, time::Duration};
use tempfile::tempdir;

fn isolated_included() -> IncludedDependencies {
    IncludedDependencies { dependencies: true, dev_dependencies: true, optional_dependencies: true }
}

/// Run the fast-path check in single-project mode with no loaded
/// lockfile and no catalogs — the shape every pre-content-check test
/// exercises.
fn check(
    workspace_root: &std::path::Path,
    config: &Config,
    node_linker: pacquet_config::NodeLinker,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
) -> Decision {
    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker,
        included: isolated_included(),
        project_manifests,
        is_workspace_install: false,
        lockfile: None,
        catalogs: &BTreeMap::default(),
    })
}

/// Write an empty `pnpm-lock.yaml` to satisfy the single-project
/// branch's lockfile-existence gate. The fast path only checks
/// existence, not contents.
fn write_empty_lockfile(workspace_root: &std::path::Path) {
    fs::write(workspace_root.join(Lockfile::FILE_NAME), "lockfileVersion: '9.0'\n")
        .expect("write pnpm-lock.yaml");
}

fn write_state(
    workspace_root: &std::path::Path,
    timestamp: i64,
    settings: WorkspaceStateSettings,
    projects: BTreeMap<String, ProjectEntry>,
) {
    let state = WorkspaceState {
        last_validated_timestamp: timestamp,
        projects,
        pnpmfiles: Vec::new(),
        filtered_install: false,
        config_dependencies: None,
        settings,
    };
    update_workspace_state(workspace_root, &state).expect("write workspace state");
}

/// Setup a workspace with a manifest written *before* the recorded
/// `lastValidatedTimestamp`. The sleep covers filesystem mtime
/// resolution (1 s on HFS+, 1 µs on APFS / ext4) so the manifest
/// reliably lands earlier in time than the state's timestamp.
fn setup_fresh_install(
    config_kind: pacquet_config::NodeLinker,
    project_name: &str,
    project_version: &str,
    manifest_extra_json: &str,
) -> (tempfile::TempDir, &'static Config, PackageManifest) {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");

    let manifest_body = if manifest_extra_json.is_empty() {
        format!(r#"{{"name":"{project_name}","version":"{project_version}"}}"#)
    } else {
        format!(
            r#"{{"name":"{project_name}","version":"{project_version}",{manifest_extra_json}}}"#,
        )
    };
    fs::write(&manifest_path, manifest_body).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    // Seed `pnpm-lock.yaml` so the single-project branch's lockfile
    // gate passes — most tests run in single-project mode (no
    // `pnpm-workspace.yaml`) and would otherwise short-circuit on
    // the missing-lockfile reason regardless of what they intend
    // to exercise.
    write_empty_lockfile(workspace_root);

    // Sleep long enough for the filesystem clock to advance past the
    // manifest's mtime before stamping the workspace state. Without
    // this, fast filesystems (APFS / tmpfs) leave both timestamps in
    // the same millisecond bucket and `<=` vs `<` flips the test.
    sleep(Duration::from_millis(20));

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    let config = Box::leak(Box::new(config));
    // Pre-create the modules dir so the "missing node_modules" guard
    // doesn't fire on the happy-path tests.
    fs::create_dir_all(&config.modules_dir).unwrap();

    let settings = current_settings(config, config_kind, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some(project_name.into()), version: Some(project_version.into()) },
    );
    write_state(workspace_root, now_millis(), settings, projects);

    (dir, config, manifest)
}

/// Happy path: state is fresh, manifest hasn't been touched since
/// the validation, modules dir exists, `pnpm-lock.yaml` exists.
/// The fast path fires.
#[test]
fn returns_up_to_date_when_state_and_manifests_agree() {
    let (dir, config, manifest) =
        setup_fresh_install(pacquet_config::NodeLinker::Isolated, "root", "1.0.0", "");

    let decision = check(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// `optimistic_repeat_install: false` opts the user out entirely.
#[test]
fn returns_skipped_when_config_disabled() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    config.optimistic_repeat_install = false;
    let config = config.leak();

    // Even though the state file is missing (would also skip), the
    // disabled-config branch is checked first — that's the reason
    // string we assert on.
    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("disabled")));
}

/// No `.pnpm-workspace-state-v1.json` on disk → cannot prove
/// freshness. Mirrors pnpm's first-return guard.
#[test]
fn returns_skipped_when_no_state_file() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    let config = config.leak();

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("no workspace state")),
    );
}

/// Manifest touched after the validation → must NOT short-circuit;
/// the regular install path needs to run.
#[test]
fn returns_skipped_when_manifest_is_newer_than_validation() {
    let (dir, config, _manifest) =
        setup_fresh_install(pacquet_config::NodeLinker::Isolated, "root", "1.0.0", "");

    // Touch the manifest after the workspace-state was stamped.
    sleep(Duration::from_millis(20));
    let manifest_path = dir.path().join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let refreshed_manifest = PackageManifest::from_path(manifest_path).unwrap();

    let decision = check(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &refreshed_manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("newer")));
}

/// Settings drift (e.g. `node_linker` changed between installs)
/// invalidates the cached state.
#[test]
fn returns_skipped_when_node_linker_drifts() {
    // Previous install was Hoisted; today's call asks for Isolated.
    let (dir, config, manifest) =
        setup_fresh_install(pacquet_config::NodeLinker::Hoisted, "root", "1.0.0", "");

    let decision = check(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Project list mismatch (cached state has a project that today's
/// walk doesn't) invalidates the cached state.
#[test]
fn returns_skipped_when_workspace_project_set_changes() {
    let (dir, config, manifest) =
        setup_fresh_install(pacquet_config::NodeLinker::Isolated, "root", "1.0.0", "");

    // Append a fake second-project entry to the cached state so
    // count + identity diverge from today's single-project walk.
    let settings =
        current_settings(config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        dir.path().join("pkg-a").to_string_lossy().into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some("1.0.0".into()) },
    );
    // Re-stamp with future timestamp so the mtime branch wouldn't
    // fire — we want to prove the project-list branch fires.
    write_state(dir.path(), now_millis() + 60_000, settings, projects);

    let decision = check(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("project list")));
}

/// Drift in `overrides` invalidates the cached state.
///
/// Ports
/// [`checkDepsStatus.test.ts:55-83`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/test/checkDepsStatus.test.ts#L55-L83)
/// `returns upToDate: false when overrides have changed`.
#[test]
fn returns_skipped_when_overrides_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("foo".to_string(), "2.0.0".to_string());
    config.overrides = Some(overrides);
    let config = config.leak();

    // Cached state has `foo: "1.0.0"` for the same key.
    let mut stale_overrides_config = Config::new();
    stale_overrides_config.modules_dir = config.modules_dir.clone();
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("foo".to_string(), "1.0.0".to_string());
    stale_overrides_config.overrides = Some(overrides);
    let stale_settings = current_settings(
        &stale_overrides_config,
        pacquet_config::NodeLinker::Isolated,
        isolated_included(),
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `injectWorkspacePackages` invalidates the cached state.
/// Toggling the flag changes whether workspace resolutions land as
/// `link:` symlinks or `file:` hard-linked copies, so the previous
/// install's virtual store no longer matches what a fresh resolution
/// would produce. Tracks pnpm/pnpm#12009 — the assertion lives here
/// so the wiring stays in place.
#[test]
fn returns_skipped_when_inject_workspace_packages_drifts() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.inject_workspace_packages = true;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.inject_workspace_packages = false;
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `enableGlobalVirtualStore` invalidates the cached state.
/// Toggling it moves the virtual store between `<storeDir>/links` and
/// each project's `node_modules/.pnpm`, so the previous install's
/// layout no longer matches a fresh resolution. Mirrors pnpm's fix for
/// [#12142](https://github.com/pnpm/pnpm/issues/12142): the toggle was
/// invisible to the freshness check until the key joined the comparison.
#[test]
fn returns_skipped_when_enable_global_virtual_store_drifts() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.enable_global_virtual_store = true;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.enable_global_virtual_store = false;
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// A pnpm-written state that records `enableGlobalVirtualStore: false`
/// (the value pnpm forces under CI) stays on the fast path for a pacquet
/// install with the store off, which omits the key. `false` and the
/// omitted `None` are the same "store off" state, so the coercion in
/// `enable_global_virtual_store_match` keeps the cross-package-manager
/// file from tripping a needless reinstall.
#[test]
fn returns_up_to_date_when_recorded_global_virtual_store_is_explicit_off() {
    let (dir, config, manifest) =
        setup_fresh_install(pacquet_config::NodeLinker::Isolated, "root", "1.0.0", "");

    let mut settings =
        current_settings(config, pacquet_config::NodeLinker::Isolated, isolated_included());
    settings.enable_global_virtual_store = Some(false);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), now_millis(), settings, projects);

    let decision = check(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// Drift in `excludeLinksFromLockfile` invalidates the cached state.
/// pnpm resolves it to a concrete `false` default and records it, so
/// pacquet must record and compare it too — otherwise pnpm's all-key
/// freshness check reports drift on every command after a pacquet
/// install.
#[test]
fn returns_skipped_when_exclude_links_from_lockfile_drifts() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.exclude_links_from_lockfile = true;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.exclude_links_from_lockfile = false;
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `minimumReleaseAge` invalidates the cached state. pnpm
/// resolves it to a concrete `1440` default and records it verbatim
/// (the raw value, not the `Some(0)`-disabled resolution), so pacquet
/// records and compares the raw value too.
#[test]
fn returns_skipped_when_minimum_release_age_drifts() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.minimum_release_age = Some(2880);
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.minimum_release_age = Some(1440);
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `minimumReleaseAgeIgnoreMissingTime` invalidates the cached
/// state. pnpm resolves it to a concrete `true` default and records it,
/// so pacquet records and compares it too.
#[test]
fn returns_skipped_when_minimum_release_age_ignore_missing_time_drifts() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.minimum_release_age_ignore_missing_time = false;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.minimum_release_age_ignore_missing_time = true;
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `ignoredOptionalDependencies` invalidates the cached
/// state.
///
/// Ports
/// [`checkDepsStatus.test.ts:115-143`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/test/checkDepsStatus.test.ts#L115-L143)
/// `returns upToDate: false when ignoredOptionalDependencies have changed`.
#[test]
fn returns_skipped_when_ignored_optional_dependencies_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.ignored_optional_dependencies = Some(vec!["new-pattern".to_string()]);
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.ignored_optional_dependencies = Some(vec!["old-pattern".to_string()]);
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `patchedDependencies` invalidates the cached state.
///
/// Ports
/// [`checkDepsStatus.test.ts:145-173`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/test/checkDepsStatus.test.ts#L145-L173)
/// `returns upToDate: false when patchedDependencies have changed`.
#[test]
fn returns_skipped_when_patched_dependencies_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let mut patched = indexmap::IndexMap::new();
    patched.insert("foo@2.0.0".to_string(), "patches/foo.patch".to_string());
    config.patched_dependencies = Some(patched);
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    let mut patched = indexmap::IndexMap::new();
    patched.insert("foo@1.0.0".to_string(), "patches/foo.patch".to_string());
    stale_config.patched_dependencies = Some(patched);
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// A patch file edited in place (same `patchedDependencies` entry, new
/// contents) invalidates the fast path. The `settings_match` key→path
/// comparison can't see a content edit, so this exercises the
/// patch-mtime branch ported from pnpm's `patchesOrHooksAreModified`.
#[test]
fn returns_skipped_when_patch_file_modified_after_validation() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let patch_path = workspace_root.join("patches").join("foo.patch");
    fs::create_dir_all(patch_path.parent().unwrap()).unwrap();
    fs::write(&patch_path, "--- a\n+++ b\n").unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let mut patched = indexmap::IndexMap::new();
    patched.insert("foo@1.0.0".to_string(), "patches/foo.patch".to_string());
    config.patched_dependencies = Some(patched);
    let config = config.leak();

    let settings =
        current_settings(config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    // Validate "now", then bump the patch's mtime past that timestamp.
    write_state(workspace_root, now_millis(), settings, projects);
    sleep(Duration::from_millis(20));
    fs::write(&patch_path, "--- a\n+++ b\n+edited\n").unwrap();

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("patch")));
}

/// An unchanged patch file (mtime older than the last validation)
/// leaves the fast path intact — the patch-mtime branch must not
/// false-positive on every install that merely configures a patch.
#[test]
fn returns_up_to_date_when_patch_file_unchanged() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let patch_path = workspace_root.join("patches").join("foo.patch");
    fs::create_dir_all(patch_path.parent().unwrap()).unwrap();
    fs::write(&patch_path, "--- a\n+++ b\n").unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let mut patched = indexmap::IndexMap::new();
    patched.insert("foo@1.0.0".to_string(), "patches/foo.patch".to_string());
    config.patched_dependencies = Some(patched);
    let config = config.leak();

    let settings =
        current_settings(config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    // Both the manifest and patch were written before this timestamp.
    sleep(Duration::from_millis(20));
    write_state(workspace_root, now_millis(), settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// Drift in `dedupePeers` invalidates the cached state. Mirrors
/// pnpm's
/// [`getOutdatedLockfileSetting` settings.dedupePeers branch](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/settings-checker/src/getOutdatedLockfileSetting.ts#L65-L67),
/// the same condition the optimistic-repeat-install gate checks here.
#[test]
fn returns_skipped_when_dedupe_peers_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.dedupe_peers = true;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.dedupe_peers = false;
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `preferWorkspacePackages` invalidates the cached state.
/// Mirrors pnpm's per-key
/// [`checkDepsStatus` settings walk](https://github.com/pnpm/pnpm/blob/180aee9ba5/deps/status/src/checkDepsStatus.ts#L138-L149)
/// — the same condition the optimistic-repeat-install gate checks
/// here.
#[test]
fn returns_skipped_when_prefer_workspace_packages_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.prefer_workspace_packages = true;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.prefer_workspace_packages = false;
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `peersSuffixMaxLength` invalidates the cached state.
///
/// Ports
/// [`checkDepsStatus.test.ts:175-203`](https://github.com/pnpm/pnpm/blob/39101f5e37/deps/status/test/checkDepsStatus.test.ts#L175-L203)
/// `returns upToDate: false when peersSuffixMaxLength has changed`.
#[test]
fn returns_skipped_when_peers_suffix_max_length_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.peers_suffix_max_length = 100;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.peers_suffix_max_length = 1000;
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `packageExtensions` invalidates the cached state.
///
/// Ports
/// [`checkDepsStatus.test.ts:85-113`](https://github.com/pnpm/pnpm/blob/39101f5e37/deps/status/test/checkDepsStatus.test.ts#L85-L113)
/// `returns upToDate: false when packageExtensions have changed`.
#[test]
fn returns_skipped_when_package_extensions_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut deps = std::collections::BTreeMap::new();
    deps.insert("dep-a".to_string(), "1.0.0".to_string());
    let extension =
        pacquet_config::PackageExtension { dependencies: Some(deps), ..Default::default() };
    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let mut extensions = indexmap::IndexMap::new();
    extensions.insert("foo".to_string(), extension);
    config.package_extensions = Some(extensions);
    let config = config.leak();

    // Cached state recorded a different `dep-a` version for `foo`.
    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    let mut deps = std::collections::BTreeMap::new();
    deps.insert("dep-a".to_string(), "2.0.0".to_string());
    let mut extensions = indexmap::IndexMap::new();
    extensions.insert(
        "foo".to_string(),
        pacquet_config::PackageExtension { dependencies: Some(deps), ..Default::default() },
    );
    stale_config.package_extensions = Some(extensions);
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `allowBuilds` invalidates the cached state.
///
/// Ports
/// [`checkDepsStatus.test.ts:205-232`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/test/checkDepsStatus.test.ts#L205-L232)
/// `returns upToDate: false when allowBuilds have changed`.
#[test]
fn returns_skipped_when_allow_builds_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.allow_builds.insert("foo".to_string(), true);
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.allow_builds.insert("foo".to_string(), false);
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `dedupeDirectDeps` invalidates the cached state. The
/// setting steers which symlinks each non-root workspace project's
/// `node_modules/` ends up with — flipping it changes the on-disk
/// shape, so the fast path can't reuse the previous install.
#[test]
fn returns_skipped_when_dedupe_direct_deps_drifts() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.dedupe_direct_deps = true;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.dedupe_direct_deps = false;
    let stale_settings =
        current_settings(&stale_config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// State written by pnpm with a field pacquet doesn't read or
/// consume during install (e.g. `packageExtensions`,
/// `excludeLinksFromLockfile`) does NOT trip the settings-drift gate.
/// Pacquet ignores those fields because its install pipeline
/// doesn't react to them — invalidating the fast path on a value
/// pacquet can't actually consume would force a redundant reinstall
/// every time a user runs `pacquet install` after `pnpm install` in
/// the same project, which is the scenario the vlt benchmark
/// exercises (pnpm/pnpm#11992).
///
/// As each setting is ported end-to-end (yaml plumbing, `Config`
/// field, real consumer, and joined into `current_settings`), it
/// joins [`settings_match`]'s comparison automatically and a
/// drift on it starts rejecting again. Tracked in pnpm/pnpm#12009.
#[test]
fn returns_up_to_date_when_state_carries_unported_pnpm_settings() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let config = config.leak();

    let mut settings =
        current_settings(config, pacquet_config::NodeLinker::Isolated, isolated_included());
    // Populate fields pacquet records but `settings_match` does not
    // compare, to prove a difference on them keeps the fast path.
    // `catalogs` is always ignored by pnpm itself; pacquet mirrors that.
    settings.catalogs = Some(serde_json::json!({"default": {"react": "^18.0.0"}}));
    // `workspacePackagePatterns` is recorded by pnpm from
    // pnpm-workspace.yaml's `packages:` field, which pacquet
    // tracks via `WorkspaceManifest.packages` instead of this
    // state-file field.
    settings.workspace_package_patterns = Some(vec!["packages/**/*".to_string()]);

    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// `allowBuilds` is the one field where pnpm and pacquet round-trip
/// an empty configured value differently: pnpm writes `Some({})` for
/// an empty allow-list, while pacquet's [`current_settings`] writes
/// `None`. The comparison must treat the two as equivalent —
/// otherwise the cross-package-manager scenario from pnpm/pnpm#11992
/// rejects the fast path on every iteration where pnpm wrote the
/// state. Mirrors pnpm's [`opts.allowBuilds ?? {}`](https://github.com/pnpm/pnpm/blob/72d997cc34/deps/status/src/checkDepsStatus.ts#L141)
/// coercion on the read side.
#[test]
fn returns_up_to_date_when_state_has_empty_allow_builds_and_current_has_none() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let config = config.leak();

    let mut settings =
        current_settings(config, pacquet_config::NodeLinker::Isolated, isolated_included());
    // Simulate a pnpm-written state: empty `allowBuilds` map
    // serialized as `{}`, where pacquet would have written `None`.
    settings.allow_builds = Some(BTreeMap::new());

    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis() + 60_000, settings, projects);

    let decision = check(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// Workspace install where a sibling project declares dependencies
/// but its `node_modules` is missing → not up to date. Mirrors
/// upstream's
/// `Workspace package X has dependencies but does not have a modules directory`.
///
/// The check only matters for sibling projects: the root's state
/// file lives inside `<workspace_root>/node_modules`, so a missing
/// root `node_modules` already trips the earlier "no workspace
/// state" guard.
#[test]
fn returns_skipped_when_sibling_node_modules_missing_for_project_with_deps() {
    let (dir, config, root_manifest) =
        setup_fresh_install(pacquet_config::NodeLinker::Isolated, "root", "1.0.0", "");

    // Add a sibling project with dependencies but no node_modules.
    let sibling_dir = dir.path().join("pkg-a");
    fs::create_dir_all(&sibling_dir).unwrap();
    let sibling_manifest_path = sibling_dir.join("package.json");
    fs::write(
        &sibling_manifest_path,
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
    )
    .unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_manifest_path).unwrap();

    // Re-stamp the workspace state with BOTH projects so the
    // project-structure check passes; use a future timestamp so the
    // mtime branch is satisfied. We want the modules-dir branch to
    // be the deciding factor.
    let settings =
        current_settings(config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        sibling_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), now_millis() + 60_000, settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pacquet_config::NodeLinker::Isolated,
        included: isolated_included(),
        project_manifests: &[
            (dir.path().to_path_buf(), &root_manifest),
            (sibling_dir, &sibling_manifest),
        ],
        is_workspace_install: true,
        lockfile: None,
        catalogs: &BTreeMap::default(),
    });
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("node_modules")));
}

/// Regression: a single-project install with `node_modules` present
/// but no `pnpm-lock.yaml` on disk must NOT short-circuit. Mirrors
/// pnpm's [single-project branch](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L396-L401)
/// throwing `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND`, which the outer
/// `try`/`catch` converts into `upToDate: false`. Without this gate,
/// pacquet's fast path fires whenever the workspace-state file and
/// manifests agree — independent of whether the lockfile exists —
/// which silently turns `pnpm.io`'s `cache+node_modules` and
/// `node_modules`-only benchmark scenarios into a 35 ms no-op.
#[test]
fn returns_skipped_when_lockfile_missing_in_single_project_mode() {
    let (dir, config, manifest) =
        setup_fresh_install(pacquet_config::NodeLinker::Isolated, "root", "1.0.0", "");

    // `setup_fresh_install` seeds `pnpm-lock.yaml` for happy-path
    // tests; delete it here so this test exercises the missing-
    // lockfile branch.
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).expect("remove seeded lockfile");

    let decision = check(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("wanted lockfile")),
        "expected Skipped(wanted lockfile missing), got {decision:?}",
    );
}

/// Workspace installs do NOT require `pnpm-lock.yaml` on disk for
/// the fast path — pnpm's
/// [workspace branch](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L268-L271)
/// returns `upToDate: true` purely off the per-manifest mtime check
/// without any wanted-lockfile probe (its merge-conflict scan,
/// `findConflictedLockfileDir`, silently `continue`s on ENOENT at
/// <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L593-L596>).
/// Pacquet must match that polarity so a workspace install state
/// file written by either tool round-trips through the other.
#[test]
fn returns_up_to_date_in_workspace_mode_without_lockfile() {
    let (dir, config, manifest) =
        setup_fresh_install(pacquet_config::NodeLinker::Isolated, "root", "1.0.0", "");

    // Same seeded state as the happy path, but the lockfile gets
    // wiped first — the workspace branch shouldn't care.
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).expect("remove seeded lockfile");

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pacquet_config::NodeLinker::Isolated,
        included: isolated_included(),
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: true,
        lockfile: None,
        catalogs: &BTreeMap::default(),
    });
    assert_eq!(decision, Decision::UpToDate);
}

/// Minimal valid lockfile matching a manifest with
/// `"dependencies": {"foo": "^1.0.0"}`.
const FOO_LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  foo@1.0.0:
    resolution: {integrity: sha512-aaa}

snapshots:

  foo@1.0.0: {}
";

const FOO_MANIFEST: &str = r#"{"name":"root","version":"1.0.0","dependencies":{"foo":"^1.0.0"}}"#;

/// Build a single project whose manifest, wanted lockfile, and current
/// lockfile all agree, with the workspace state stamped after every
/// file write. Content-check tests then touch or rewrite individual
/// files and assert the decision.
fn setup_content_check_project() -> (tempfile::TempDir, &'static Config) {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    fs::write(workspace_root.join("package.json"), FOO_MANIFEST).unwrap();
    fs::write(workspace_root.join(Lockfile::FILE_NAME), FOO_LOCKFILE).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    config.virtual_store_dir = workspace_root.join("node_modules/.pnpm");
    fs::create_dir_all(&config.virtual_store_dir).unwrap();
    fs::write(config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME), FOO_LOCKFILE).unwrap();
    let config = config.leak();

    sleep(Duration::from_millis(20));
    let settings =
        current_settings(config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, now_millis(), settings, projects);
    sleep(Duration::from_millis(20));

    (dir, config)
}

fn content_check_decision(
    dir: &tempfile::TempDir,
    config: &'static Config,
    is_workspace_install: bool,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
) -> Decision {
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).expect("parse pnpm-lock.yaml");
    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pacquet_config::NodeLinker::Isolated,
        included: isolated_included(),
        project_manifests,
        is_workspace_install,
        lockfile: lockfile.as_ref(),
        catalogs: &BTreeMap::default(),
    })
}

/// A manifest rewrite that leaves the dependency fields intact — the
/// shape `touch package.json` / `npm pkg set/delete` produce — must
/// still short-circuit. Ports the contract behind upstream's
/// `assertWantedLockfileUpToDate` pass at
/// <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L420-L447>.
#[test]
fn returns_up_to_date_when_touched_manifest_still_satisfies_lockfile() {
    let (dir, config) = setup_content_check_project();

    fs::write(dir.path().join("package.json"), FOO_MANIFEST).unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(decision, Decision::UpToDate);
}

/// A manifest rewrite that *changes* the dependency fields falls
/// through to the full install.
#[test]
fn returns_skipped_when_touched_manifest_adds_a_dependency() {
    let (dir, config) = setup_content_check_project();

    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"root","version":"1.0.0","dependencies":{"foo":"^1.0.0","bar":"^2.0.0"}}"#,
    )
    .unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("satisfied")),
        "expected Skipped(no longer satisfied), got {decision:?}",
    );
}

/// A wanted lockfile rewritten after the last install (newer than the
/// current lockfile, different content) cannot short-circuit: the
/// modules directory no longer reflects it. Ports upstream's
/// [`assertLockfilesEqual`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/assertLockfilesEqual.ts)
/// `RUN_CHECK_DEPS_OUTDATED_DEPS` outcome.
#[test]
fn returns_skipped_when_wanted_lockfile_diverged_from_current() {
    let (dir, config) = setup_content_check_project();

    fs::write(dir.path().join("package.json"), FOO_MANIFEST).unwrap();
    fs::write(dir.path().join(Lockfile::FILE_NAME), FOO_LOCKFILE.replace("1.0.0", "1.0.1"))
        .unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("not up to date")),
        "expected Skipped(outdated deps), got {decision:?}",
    );
}

/// Workspace branch: a passing content check refreshes
/// `lastValidatedTimestamp` so the next run exits on the pure-mtime
/// path. Mirrors upstream's `updateWorkspaceState` call at
/// <https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/status/src/checkDepsStatus.ts#L349-L357>.
#[test]
fn workspace_content_check_refreshes_last_validated_timestamp() {
    let (dir, config) = setup_content_check_project();
    let before = pacquet_workspace_state::load_workspace_state(dir.path())
        .unwrap()
        .unwrap()
        .last_validated_timestamp;

    fs::write(dir.path().join("package.json"), FOO_MANIFEST).unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, true, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(decision, Decision::UpToDate);

    let after = pacquet_workspace_state::load_workspace_state(dir.path())
        .unwrap()
        .unwrap()
        .last_validated_timestamp;
    assert!(after > before, "expected the state timestamp to advance ({before} -> {after})");
}

/// Workspace lockfile whose root importer links a sibling: the link
/// stays valid while the sibling's version satisfies the manifest
/// range, and a bump outside the range falls through to the full
/// install. Ports upstream's
/// [`linkedPackagesAreUpToDate`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/lockfile/verification/src/linkedPackagesAreUpToDate.ts).
fn linked_sibling_decision(sibling_version: &str) -> Decision {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    fs::write(
        workspace_root.join("package.json"),
        r#"{"name":"root","version":"1.0.0","devDependencies":{"pkg-a":"^1.0.0"}}"#,
    )
    .unwrap();
    let sibling_dir = workspace_root.join("pkg-a");
    fs::create_dir_all(&sibling_dir).unwrap();
    fs::write(
        sibling_dir.join("package.json"),
        format!(r#"{{"name":"pkg-a","version":"{sibling_version}"}}"#),
    )
    .unwrap();
    fs::write(
        workspace_root.join(Lockfile::FILE_NAME),
        "lockfileVersion: '9.0'

importers:

  .:
    devDependencies:
      pkg-a:
        specifier: ^1.0.0
        version: link:pkg-a

  pkg-a: {}
",
    )
    .unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    config.virtual_store_dir = workspace_root.join("node_modules/.pnpm");
    config.link_workspace_packages = pacquet_config::LinkWorkspacePackages::DirectOnly;
    fs::create_dir_all(&config.modules_dir).unwrap();
    let config = config.leak();

    let root_manifest = PackageManifest::from_path(workspace_root.join("package.json")).unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_dir.join("package.json")).unwrap();

    sleep(Duration::from_millis(20));
    let settings =
        current_settings(config, pacquet_config::NodeLinker::Isolated, isolated_included());
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        sibling_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some(sibling_version.into()) },
    );
    write_state(workspace_root, now_millis(), settings, projects);
    sleep(Duration::from_millis(20));

    // Touch the root manifest so the content re-check runs.
    fs::write(
        workspace_root.join("package.json"),
        r#"{"name":"root","version":"1.0.0","devDependencies":{"pkg-a":"^1.0.0"}}"#,
    )
    .unwrap();
    let root_manifest_touched =
        PackageManifest::from_path(workspace_root.join("package.json")).unwrap();
    let _ = root_manifest;

    let lockfile = Lockfile::load_wanted_from_dir(workspace_root).expect("parse pnpm-lock.yaml");
    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker: pacquet_config::NodeLinker::Isolated,
        included: isolated_included(),
        project_manifests: &[
            (workspace_root.to_path_buf(), &root_manifest_touched),
            (sibling_dir, &sibling_manifest),
        ],
        is_workspace_install: true,
        lockfile: lockfile.as_ref(),
        catalogs: &BTreeMap::default(),
    })
}

#[test]
fn returns_up_to_date_when_linked_sibling_still_satisfies_range() {
    assert_eq!(linked_sibling_decision("1.5.0"), Decision::UpToDate);
}

#[test]
fn returns_skipped_when_linked_sibling_no_longer_satisfies_range() {
    let decision = linked_sibling_decision("2.0.0");
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("linked")),
        "expected Skipped(linked package out of date), got {decision:?}",
    );
}

/// `pnpm-lock.yaml` deleted while `node_modules` (and its current
/// lockfile) is intact: the current lockfile stands in as the wanted
/// one, and the fast path regenerates `pnpm-lock.yaml` from it instead
/// of falling into the full install pipeline.
#[test]
fn regenerates_missing_wanted_lockfile_from_current_when_manifests_unchanged() {
    let (dir, config) = setup_content_check_project();
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(decision, Decision::UpToDate);

    let regenerated = Lockfile::load_wanted_from_dir(dir.path())
        .expect("parse regenerated pnpm-lock.yaml")
        .expect("pnpm-lock.yaml must be regenerated from the current lockfile");
    let current =
        Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir).unwrap().unwrap();
    assert_eq!(regenerated, current);
}

/// Same as above with a touched (content-identical) manifest — the
/// content re-check runs against the current lockfile.
#[test]
fn regenerates_missing_wanted_lockfile_when_touched_manifest_satisfies_current() {
    let (dir, config) = setup_content_check_project();
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).unwrap();
    fs::write(dir.path().join("package.json"), FOO_MANIFEST).unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(decision, Decision::UpToDate);
    assert!(dir.path().join(Lockfile::FILE_NAME).exists(), "pnpm-lock.yaml must be regenerated");
}

/// A manifest that no longer matches the current lockfile cannot ride
/// the current-as-wanted fallback — the full install must resolve.
#[test]
fn returns_skipped_when_missing_wanted_lockfile_and_manifest_adds_a_dependency() {
    let (dir, config) = setup_content_check_project();
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"root","version":"1.0.0","dependencies":{"foo":"^1.0.0","bar":"^2.0.0"}}"#,
    )
    .unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("satisfied")),
        "expected Skipped(no longer satisfied), got {decision:?}",
    );
    assert!(
        !dir.path().join(Lockfile::FILE_NAME).exists(),
        "must not regenerate on a failed check",
    );
}

/// Workspace mode: deleted `pnpm-lock.yaml` + touched manifest takes
/// the current-as-wanted fallback, regenerates the lockfile, and
/// refreshes the state timestamp.
#[test]
fn workspace_regenerates_missing_wanted_lockfile_and_bumps_state() {
    let (dir, config) = setup_content_check_project();
    let before = pacquet_workspace_state::load_workspace_state(dir.path())
        .unwrap()
        .unwrap()
        .last_validated_timestamp;
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).unwrap();
    fs::write(dir.path().join("package.json"), FOO_MANIFEST).unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, true, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(decision, Decision::UpToDate);
    assert!(dir.path().join(Lockfile::FILE_NAME).exists(), "pnpm-lock.yaml must be regenerated");
    let after = pacquet_workspace_state::load_workspace_state(dir.path())
        .unwrap()
        .unwrap()
        .last_validated_timestamp;
    assert!(after > before, "expected the state timestamp to advance ({before} -> {after})");
}

/// `lockfile: false` (pnpm's `useLockfile: false`) disables the
/// regeneration but keeps the fast path.
#[test]
fn does_not_regenerate_wanted_lockfile_when_lockfile_writing_disabled() {
    let (dir, config) = setup_content_check_project();
    // `Config` is leaked per test; build a second one with `lockfile`
    // off instead of mutating the shared reference.
    let mut no_lockfile_config = Config::new();
    no_lockfile_config.modules_dir = config.modules_dir.clone();
    no_lockfile_config.virtual_store_dir = config.virtual_store_dir.clone();
    no_lockfile_config.lockfile = false;
    let no_lockfile_config = no_lockfile_config.leak();
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision = content_check_decision(
        &dir,
        no_lockfile_config,
        false,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
    assert!(!dir.path().join(Lockfile::FILE_NAME).exists(), "lockfile: false must skip the write");
}
