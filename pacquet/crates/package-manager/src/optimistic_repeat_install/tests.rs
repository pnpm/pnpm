use super::{Decision, check_optimistic_repeat_install};
use pacquet_config::Config;
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

/// Build the [`WorkspaceStateSettings`] today's install would write,
/// so the cached state matches by default and the freshness check
/// reaches the mtime branch.
fn current_settings(
    config: &Config,
    node_linker: pacquet_config::NodeLinker,
    included: IncludedDependencies,
) -> WorkspaceStateSettings {
    use pacquet_config::LinkWorkspacePackages;
    use pacquet_workspace_state::NodeLinker as WSNodeLinker;
    let allow_builds = (!config.allow_builds.is_empty()).then(|| {
        config.allow_builds.iter().map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v))).collect()
    });
    let lwp = match config.link_workspace_packages {
        LinkWorkspacePackages::Off => serde_json::Value::Bool(false),
        LinkWorkspacePackages::DirectOnly => serde_json::Value::Bool(true),
        LinkWorkspacePackages::Deep => serde_json::Value::String("deep".to_string()),
    };
    let node_linker = match node_linker {
        pacquet_config::NodeLinker::Isolated => WSNodeLinker::Isolated,
        pacquet_config::NodeLinker::Hoisted => WSNodeLinker::Hoisted,
        pacquet_config::NodeLinker::Pnp => WSNodeLinker::Pnp,
    };
    WorkspaceStateSettings {
        allow_builds,
        auto_install_peers: Some(config.auto_install_peers),
        dedupe_peer_dependents: Some(config.dedupe_peer_dependents),
        dev: Some(included.dev_dependencies),
        hoist_pattern: config.hoist_pattern.clone(),
        hoist_workspace_packages: Some(config.hoist_workspace_packages),
        ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
        link_workspace_packages: Some(lwp),
        node_linker: Some(node_linker),
        optional: Some(included.optional_dependencies),
        overrides: config
            .overrides
            .as_ref()
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        patched_dependencies: config.patched_dependencies.clone(),
        production: Some(included.dependencies),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        ..Default::default()
    }
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
            r#"{{"name":"{project_name}","version":"{project_version}",{manifest_extra_json}}}"#
        )
    };
    fs::write(&manifest_path, manifest_body).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

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
/// the validation, modules dir exists. The fast path fires.
#[test]
fn returns_up_to_date_when_state_and_manifests_agree() {
    let (dir, config, manifest) =
        setup_fresh_install(pacquet_config::NodeLinker::Isolated, "root", "1.0.0", "");

    let decision = check_optimistic_repeat_install(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        isolated_included(),
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
    let decision = check_optimistic_repeat_install(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        isolated_included(),
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

    let decision = check_optimistic_repeat_install(
        workspace_root,
        config,
        pacquet_config::NodeLinker::Isolated,
        isolated_included(),
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("no workspace state"))
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

    let decision = check_optimistic_repeat_install(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        isolated_included(),
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

    let decision = check_optimistic_repeat_install(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        isolated_included(),
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

    let decision = check_optimistic_repeat_install(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        isolated_included(),
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("project list")));
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

    let decision = check_optimistic_repeat_install(
        dir.path(),
        config,
        pacquet_config::NodeLinker::Isolated,
        isolated_included(),
        &[(dir.path().to_path_buf(), &root_manifest), (sibling_dir, &sibling_manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("node_modules")));
}
