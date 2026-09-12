mod integrity;

mod builds;

mod store;

mod runtimes;

mod catalogs;

mod resolution;

mod lockfile;

mod workspace;

mod installation;

mod hooks;

use super::{
    Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install,
    deps_status::{RunDepsStatus, check_deps_status_before_run},
    settings::current_settings,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::{backdate_existing_files, set_mtime};
use pnpm_workspace_state::{
    ProjectEntry, WorkspaceState, WorkspaceStateSettings, load_workspace_state,
    update_workspace_state,
};
use ssri::{Algorithm, IntegrityOpts};
use std::{collections::BTreeMap, fs};
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
    node_linker: pnpm_config::NodeLinker,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
) -> Decision {
    check_with_catalogs(
        workspace_root,
        config,
        node_linker,
        project_manifests,
        false,
        &BTreeMap::default(),
    )
}

fn check_with_catalogs(
    workspace_root: &std::path::Path,
    config: &Config,
    node_linker: pnpm_config::NodeLinker,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
    is_workspace_install: bool,
    catalogs: &Catalogs,
) -> Decision {
    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests,
        is_workspace_install,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs,
    })
}

fn check_with_lockfile(
    workspace_root: &std::path::Path,
    config: &Config,
    node_linker: pnpm_config::NodeLinker,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
    lockfile: &Lockfile,
) -> Decision {
    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests,
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(Some(lockfile)),
        catalogs: &BTreeMap::default(),
    })
}

fn check_workspace(
    workspace_root: &std::path::Path,
    config: &Config,
    node_linker: pnpm_config::NodeLinker,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
    catalogs: &Catalogs,
) -> Decision {
    check_with_catalogs(workspace_root, config, node_linker, project_manifests, true, catalogs)
}

/// Write an empty `pnpm-lock.yaml` to satisfy the single-project
/// branch's lockfile-existence gate. The fast path only checks
/// existence, not contents.
fn write_empty_lockfile(workspace_root: &std::path::Path) {
    fs::write(workspace_root.join(Lockfile::FILE_NAME), "lockfileVersion: '9.0'\n")
        .expect("write pnpm-lock.yaml");
}

fn write_local_tarball_lockfile(
    workspace_root: &std::path::Path,
    virtual_store_dir: &std::path::Path,
    dependency_group: &str,
    tarball: &[u8],
) -> Lockfile {
    let integrity = IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(tarball).result();
    fs::write(
        workspace_root.join(Lockfile::FILE_NAME),
        format!(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    {dependency_group}:\n      tar:\n        specifier: file:./vendor/tar.tgz\n        version: file:vendor/tar.tgz\npackages:\n  tar@file:vendor/tar.tgz:\n    resolution: {{integrity: {integrity}, tarball: file:vendor/tar.tgz}}\nsnapshots:\n  tar@file:vendor/tar.tgz: {{}}\n",
        ),
    )
    .expect("write local tarball lockfile");
    let lockfile = Lockfile::load_wanted_from_dir(workspace_root)
        .expect("load local tarball lockfile")
        .expect("local tarball lockfile on disk");
    lockfile
        .save_current_to_virtual_store_dir(virtual_store_dir)
        .expect("write current local tarball lockfile");
    lockfile
}

fn write_bare_tarball_lockfile(
    workspace_root: &std::path::Path,
    virtual_store_dir: &std::path::Path,
    tarball: &[u8],
) -> Lockfile {
    let integrity = IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(tarball).result();
    fs::write(
        workspace_root.join(Lockfile::FILE_NAME),
        format!(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      tar:\n        specifier: dependency.tgz\n        version: file:dependency.tgz\npackages:\n  tar@file:dependency.tgz:\n    resolution: {{integrity: {integrity}, tarball: file:dependency.tgz}}\nsnapshots:\n  tar@file:dependency.tgz: {{}}\n",
        ),
    )
    .expect("write bare tarball lockfile");
    let lockfile = Lockfile::load_wanted_from_dir(workspace_root)
        .expect("load bare tarball lockfile")
        .expect("bare tarball lockfile on disk");
    lockfile
        .save_current_to_virtual_store_dir(virtual_store_dir)
        .expect("write current bare tarball lockfile");
    lockfile
}

fn write_registry_lockfile(
    workspace_root: &std::path::Path,
    virtual_store_dir: &std::path::Path,
    specifier: &str,
) -> Lockfile {
    let integrity = IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(b"registry").result();
    fs::write(
        workspace_root.join(Lockfile::FILE_NAME),
        format!(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      tar:\n        specifier: {specifier}\n        version: 1.0.0\npackages:\n  tar@1.0.0:\n    resolution: {{integrity: {integrity}}}\nsnapshots:\n  tar@1.0.0: {{}}\n",
        ),
    )
    .expect("write registry lockfile");
    let lockfile = Lockfile::load_wanted_from_dir(workspace_root)
        .expect("load registry lockfile")
        .expect("registry lockfile on disk");
    lockfile
        .save_current_to_virtual_store_dir(virtual_store_dir)
        .expect("write current registry lockfile");
    lockfile
}

fn write_state(
    workspace_root: &std::path::Path,
    timestamp: i64,
    settings: WorkspaceStateSettings,
    projects: BTreeMap<String, ProjectEntry>,
) {
    write_state_with_pnpmfiles(workspace_root, timestamp, settings, projects, Vec::new());
}

fn write_state_with_pnpmfiles(
    workspace_root: &std::path::Path,
    timestamp: i64,
    settings: WorkspaceStateSettings,
    projects: BTreeMap<String, ProjectEntry>,
    pnpmfiles: Vec<String>,
) {
    let state = WorkspaceState {
        last_validated_timestamp: timestamp,
        projects,
        pnpmfiles,
        filtered_install: false,
        config_dependencies: None,
        settings,
    };
    update_workspace_state(workspace_root, &state).expect("write workspace state");
}

fn validate_existing_files(workspace_root: &std::path::Path) {
    let timestamp = backdate_existing_files(workspace_root);
    let mut state = load_workspace_state(workspace_root)
        .expect("read workspace state")
        .expect("workspace state on disk");
    state.last_validated_timestamp = timestamp;
    update_workspace_state(workspace_root, &state).expect("refresh workspace state");
}

/// Setup a workspace whose recorded `lastValidatedTimestamp` covers
/// every file written so far, so the manifest reads as validated.
fn setup_fresh_install(
    config_kind: pnpm_config::NodeLinker,
    project_name: &str,
    project_version: &str,
    manifest_extra_json: &str,
) -> (tempfile::TempDir, &'static Config, PackageManifest) {
    setup_fresh_install_with_config(
        config_kind,
        project_name,
        project_version,
        manifest_extra_json,
        |_| {},
    )
}

/// Same as [`setup_fresh_install`] but applies `configure` to the
/// `Config` before the workspace-state snapshot is taken, so the
/// settings comparison sees the configured values as unchanged.
fn setup_fresh_install_with_config(
    config_kind: pnpm_config::NodeLinker,
    project_name: &str,
    project_version: &str,
    manifest_extra_json: &str,
    configure: impl FnOnce(&mut Config),
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

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    config.virtual_store_dir = config.modules_dir.join(".pnpm");
    configure(&mut config);
    let config = Box::leak(Box::new(config));
    // Pre-create the modules dir so the "missing node_modules" guard
    // doesn't fire on the happy-path tests.
    fs::create_dir_all(&config.modules_dir).unwrap();

    let settings = current_settings(config, config_kind, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some(project_name.into()), version: Some(project_version.into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    (dir, config, manifest)
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

const FOO_LOCKFILE_WITHOUT_PACKAGES: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
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

    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

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
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests,
        is_workspace_install,
        lockfile: MaybeLazyLockfile::Loaded(lockfile.as_ref()),
        catalogs: &BTreeMap::default(),
    })
}

/// The instant every mtime-collision test stamps on the files the
/// freshness check stats and on the recorded `lastValidatedTimestamp`.
const COLLIDING_MTIME_SECS: u64 = 1_700_000_000;
/// [`COLLIDING_MTIME_SECS`] in the milliseconds the state records.
const COLLIDING_MTIME_MS: i64 = 1_700_000_000_000;

/// Reproduce the mtime-tick collision of
/// [#13907](https://github.com/pnpm/pnpm/issues/13907): every file the
/// freshness check stats is stamped `subsec_nanos` into the millisecond
/// the workspace state records as validated, so the manifest reads as
/// possibly-modified and the content check runs. A `subsec_nanos` of
/// zero is the whole-second-mtime filesystem's version of the same
/// collision.
fn collide_mtimes_with_recorded_state(
    workspace_root: &std::path::Path,
    config: &Config,
    subsec_nanos: u32,
) {
    let modified = std::time::SystemTime::UNIX_EPOCH
        + std::time::Duration::new(COLLIDING_MTIME_SECS, subsec_nanos);
    for path in [
        workspace_root.join("package.json"),
        workspace_root.join(Lockfile::FILE_NAME),
        config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME),
    ] {
        set_mtime(&path, modified);
    }
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(
        workspace_root,
        COLLIDING_MTIME_MS,
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None),
        projects,
    );
}

/// Rewrite the wanted lockfile so a content check would report the
/// project outdated while leaving its mtime untouched, so an up-to-date
/// verdict after this can only have come from the pure-mtime fast path.
fn poison_lockfile_content(workspace_root: &std::path::Path) {
    let path = workspace_root.join(Lockfile::FILE_NAME);
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, FOO_LOCKFILE.replace("1.0.0", "1.0.1")).unwrap();
    set_mtime(&path, modified);
}

fn recorded_timestamp(workspace_root: &std::path::Path) -> i64 {
    load_workspace_state(workspace_root)
        .expect("read the workspace state")
        .expect("a workspace state to have been written")
        .last_validated_timestamp
}

fn assert_content_check_converges_after_collision(subsec_nanos: u32) {
    let (dir, config) = setup_content_check_project();
    collide_mtimes_with_recorded_state(dir.path(), config, subsec_nanos);
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
    let project_manifests = [(dir.path().to_path_buf(), &manifest)];

    assert_eq!(content_check_decision(&dir, config, true, &project_manifests), Decision::UpToDate);
    let refreshed = recorded_timestamp(dir.path());
    assert!(
        refreshed > COLLIDING_MTIME_MS,
        "expected the passing content check to advance the baseline past {COLLIDING_MTIME_MS}, got {refreshed}",
    );

    poison_lockfile_content(dir.path());
    assert_eq!(content_check_decision(&dir, config, true, &project_manifests), Decision::UpToDate);
    assert_eq!(recorded_timestamp(dir.path()), refreshed);
}

fn workspace_deps_status(
    dir: &tempfile::TempDir,
    config: &'static Config,
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
) -> RunDepsStatus {
    let state = load_workspace_state(dir.path())
        .expect("read the workspace state")
        .expect("a workspace state to have been written");
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).expect("parse pnpm-lock.yaml");
    check_deps_status_before_run(
        &OptimisticRepeatInstallCheck {
            workspace_root: dir.path(),
            config,
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
            project_manifests,
            is_workspace_install: true,
            lockfile: MaybeLazyLockfile::Loaded(lockfile.as_ref()),
            catalogs: &BTreeMap::default(),
        },
        &state,
    )
}

fn assert_deps_status_converges_after_collision(subsec_nanos: u32) {
    let (dir, config) = setup_content_check_project();
    collide_mtimes_with_recorded_state(dir.path(), config, subsec_nanos);
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
    let project_manifests = [(dir.path().to_path_buf(), &manifest)];

    assert_eq!(workspace_deps_status(&dir, config, &project_manifests), RunDepsStatus::UpToDate);
    let refreshed = recorded_timestamp(dir.path());
    assert!(
        refreshed > COLLIDING_MTIME_MS,
        "expected the passing content check to advance the baseline past {COLLIDING_MTIME_MS}, got {refreshed}",
    );

    poison_lockfile_content(dir.path());
    assert_eq!(workspace_deps_status(&dir, config, &project_manifests), RunDepsStatus::UpToDate);
    assert_eq!(recorded_timestamp(dir.path()), refreshed);
}

/// Workspace lockfile whose root importer links a sibling: the link
/// stays valid while the sibling's version satisfies the manifest
/// range, and a bump outside the range falls through to the full
/// install.
fn linked_sibling_decision(sibling_version: &str) -> Decision {
    linked_sibling_decision_for_spec(
        "pkg-a",
        "^1.0.0",
        "link:pkg-a",
        sibling_version,
        pnpm_config::LinkWorkspacePackages::DirectOnly,
    )
}

fn linked_sibling_decision_for_spec(
    dependency_name: &str,
    specifier: &str,
    lockfile_ref: &str,
    sibling_version: &str,
    link_workspace_packages: pnpm_config::LinkWorkspacePackages,
) -> Decision {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    fs::write(
        workspace_root.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "devDependencies": { (dependency_name): specifier },
        })
        .to_string(),
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
        format!(
            "lockfileVersion: '9.0'

importers:

  .:
    devDependencies:
      {dependency_name}:
        specifier: {specifier}
        version: {lockfile_ref}

  pkg-a: {{}}
",
        ),
    )
    .unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    config.virtual_store_dir = workspace_root.join("node_modules/.pnpm");
    config.link_workspace_packages = link_workspace_packages;
    fs::create_dir_all(&config.modules_dir).unwrap();
    let config = config.leak();

    let root_manifest = PackageManifest::from_path(workspace_root.join("package.json")).unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_dir.join("package.json")).unwrap();

    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        sibling_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some(sibling_version.into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    // Touch the root manifest so the content re-check runs.
    fs::write(
        workspace_root.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "devDependencies": { (dependency_name): specifier },
        })
        .to_string(),
    )
    .unwrap();
    let root_manifest_touched =
        PackageManifest::from_path(workspace_root.join("package.json")).unwrap();
    let _ = root_manifest;

    let lockfile = Lockfile::load_wanted_from_dir(workspace_root).expect("parse pnpm-lock.yaml");
    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root,
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests: &[
            (workspace_root.to_path_buf(), &root_manifest_touched),
            (sibling_dir, &sibling_manifest),
        ],
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(lockfile.as_ref()),
        catalogs: &BTreeMap::default(),
    })
}
