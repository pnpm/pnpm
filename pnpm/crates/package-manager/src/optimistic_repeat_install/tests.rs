use super::{
    Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install,
    check_optimistic_repeat_install_ignoring,
    conflict_markers::MAX_LOCKFILE_CONFLICT_SCAN_BYTES,
    current_pnpmfiles,
    deps_status::{RunDepsStatus, check_deps_status_before_run, install_args_from_state},
    manifest_agreement::{LinkedPackagesContext, linked_packages_are_up_to_date},
    settings::{current_settings, current_settings_with_catalogs},
    timestamps::{FileMtime, lockfile_modified_since, modified_at_or_after},
};
use indexmap::IndexMap;
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

#[test]
fn returns_skipped_when_a_pnpmfile_is_modified() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");
    let pnpmfile = dir.path().join(".pnpmfile.cjs");
    fs::write(&pnpmfile, "module.exports = {}\n").expect("write pnpmfile");
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state_with_pnpmfiles(
        dir.path(),
        backdate_existing_files(dir.path()),
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None),
        projects,
        current_pnpmfiles(dir.path(), config),
    );
    fs::write(&pnpmfile, "module.exports = { hooks: {} }\n").expect("modify pnpmfile");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );

    assert!(matches!(
        decision,
        Decision::Skipped { reason } if reason.contains("pnpmfile changed")
    ));
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

/// Happy path: state is fresh, manifest hasn't been touched since
/// the validation, modules dir exists, `pnpm-lock.yaml` exists.
/// The fast path fires.
#[test]
fn returns_up_to_date_when_state_and_manifests_agree() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// A filtered install refreshes `lastValidatedTimestamp` while leaving
/// the projects it did not select untouched, so its state cannot prove
/// the workspace is current: the next install re-validates for real.
#[test]
fn returns_skipped_when_the_previous_install_was_filtered() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");
    let mut state = load_workspace_state(dir.path()).expect("read state").expect("state on disk");
    state.filtered_install = true;
    update_workspace_state(dir.path(), &state).expect("write workspace state");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );

    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("previous install was filtered")),
        "unexpected decision: {decision:?}",
    );
}

/// A `file:` dependency must never short-circuit: nothing the fast
/// path stats covers the dependency's *contents*, so the full install
/// path has to run and refetch it.
#[test]
fn returns_skipped_when_a_project_has_a_file_dependency() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"file:../foo"}"#,
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
    );
}

/// A missing local tarball must reach the full install path so it can report
/// the resolver's contextual error.
#[test]
fn returns_skipped_when_a_project_has_a_missing_file_tarball_dependency() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""devDependencies":{"tar":"file:./vendor/tar.tgz"}"#,
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
    );
}

#[test]
fn returns_up_to_date_when_a_project_has_an_unchanged_file_tarball_dependency() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""devDependencies":{"tar":"file:./vendor/tar.tgz"}"#,
    );
    fs::create_dir_all(dir.path().join("vendor")).expect("create vendor dir");
    let tarball = b"unchanged";
    fs::write(dir.path().join("vendor/tar.tgz"), tarball).expect("write tarball");
    let lockfile = write_local_tarball_lockfile(
        dir.path(),
        &config.virtual_store_dir,
        "devDependencies",
        tarball,
    );
    validate_existing_files(dir.path());

    let decision = check_with_lockfile(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
        &lockfile,
    );

    assert_eq!(decision, Decision::UpToDate);
}

#[test]
fn returns_skipped_when_a_project_file_tarball_changed_after_validation() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"tar":"file:./vendor/tar.tgz"}"#,
    );
    fs::create_dir_all(dir.path().join("vendor")).expect("create vendor dir");
    let tarball = dir.path().join("vendor/tar.tgz");
    fs::write(&tarball, b"original").expect("write tarball");
    let lockfile = write_local_tarball_lockfile(
        dir.path(),
        &config.virtual_store_dir,
        "dependencies",
        b"original",
    );
    validate_existing_files(dir.path());
    fs::write(&tarball, b"repacked").expect("repack tarball");

    let decision = check_with_lockfile(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
        &lockfile,
    );

    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
    );
}

#[test]
fn returns_skipped_when_a_project_file_tarball_changes_without_an_mtime_change() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"tar":"file:./vendor/tar.tgz"}"#,
    );
    fs::create_dir_all(dir.path().join("vendor")).expect("create vendor dir");
    let tarball = dir.path().join("vendor/tar.tgz");
    fs::write(&tarball, b"original").expect("write tarball");
    let modified = fs::metadata(&tarball).expect("stat tarball").modified().expect("tarball mtime");
    let lockfile = write_local_tarball_lockfile(
        dir.path(),
        &config.virtual_store_dir,
        "dependencies",
        b"original",
    );
    validate_existing_files(dir.path());
    fs::write(&tarball, b"repacked").expect("repack tarball");
    set_mtime(&tarball, modified);

    let decision = check_with_lockfile(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
        &lockfile,
    );

    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
    );
}

#[test]
fn returns_skipped_when_a_file_tarball_spec_points_to_a_directory() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"tar":"file:./vendor/tar.tgz"}"#,
    );
    fs::create_dir_all(dir.path().join("vendor/tar.tgz")).expect("create tarball-shaped dir");
    validate_existing_files(dir.path());

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );

    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
    );
}

#[test]
fn returns_up_to_date_when_bare_tarball_specs_are_ambiguous() {
    for spec in ["dependency.tgz", "user/repo.tgz"] {
        let (dir, config, manifest) = setup_fresh_install(
            pnpm_config::NodeLinker::Isolated,
            "root",
            "1.0.0",
            &format!(r#""dependencies":{{"tar":"{spec}"}}"#),
        );
        let path = dir.path().join(spec);
        fs::create_dir_all(path.parent().expect("tarball parent")).expect("create parent");
        fs::write(path, b"local file with an ambiguous specifier").expect("write local file");
        let lockfile = write_registry_lockfile(dir.path(), &config.virtual_store_dir, spec);
        validate_existing_files(dir.path());

        let decision = check_with_lockfile(
            dir.path(),
            config,
            pnpm_config::NodeLinker::Isolated,
            &[(dir.path().to_path_buf(), &manifest)],
            &lockfile,
        );
        assert_eq!(decision, Decision::UpToDate, "specifier {spec:?}");
    }
}

#[test]
fn verifies_a_bare_tarball_when_the_lockfile_records_a_local_resolution() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"tar":"dependency.tgz"}"#,
    );
    let tarball = dir.path().join("dependency.tgz");
    fs::write(&tarball, b"original").expect("write bare tarball");
    let lockfile = write_bare_tarball_lockfile(dir.path(), &config.virtual_store_dir, b"original");
    validate_existing_files(dir.path());

    let unchanged = check_with_lockfile(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
        &lockfile,
    );
    assert_eq!(unchanged, Decision::UpToDate);

    fs::write(&tarball, b"repacked").expect("repack bare tarball");
    let changed = check_with_lockfile(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
        &lockfile,
    );
    assert!(
        matches!(changed, Decision::Skipped { reason } if reason.contains("local file dependency")),
    );
}

/// Bare local paths resolve to local directory dependencies and stay on the
/// full install path.
#[test]
fn returns_skipped_when_a_project_has_a_bare_local_path_dependency() {
    for spec in ["../sibling-dir", "~/pkgs/foo", "/abs/path/foo", "c:/pkgs/foo", "c:pkgs"] {
        let (dir, config, manifest) = setup_fresh_install(
            pnpm_config::NodeLinker::Isolated,
            "root",
            "1.0.0",
            &format!(r#""dependencies":{{"foo":"{spec}"}}"#),
        );

        let decision = check(
            dir.path(),
            config,
            pnpm_config::NodeLinker::Isolated,
            &[(dir.path().to_path_buf(), &manifest)],
        );
        assert!(
            matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
            "spec {spec:?} must bail",
        );
    }
}

/// A local file dependency in a group excluded from the install must
/// not bail: the group isn't materialized, so its contents can't be
/// stale. A change to the include flags themselves is caught by the
/// settings comparison instead.
#[test]
fn returns_up_to_date_when_the_local_file_dependency_is_in_an_excluded_group() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""optionalDependencies":{"foo":"file:../foo"}"#,
    );

    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: false,
    };
    // Re-stamp the state with the same include flags the check runs
    // under, so the settings comparison passes and the include gate is
    // what gets exercised.
    let settings = current_settings(config, pnpm_config::NodeLinker::Isolated, included, None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_existing_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included,
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
    });
    assert_eq!(decision, Decision::UpToDate);
}

/// The include gate is per-group: a local file dependency in a group
/// that *is* installed still bails even when other groups are excluded.
#[test]
fn returns_skipped_when_the_local_file_dependency_is_in_an_included_group() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"file:../foo"}"#,
    );

    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };
    let settings = current_settings(config, pnpm_config::NodeLinker::Isolated, included, None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_existing_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included,
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
    });
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
    );
}

/// A `pnpm.overrides` entry mapping to a local file spec must bail the
/// same way a direct local file dependency does: the override redirects
/// every matching dependency in the graph to that directory, and
/// nothing the fast path stats covers its contents.
#[test]
fn returns_skipped_when_an_override_maps_to_a_local_file_dependency() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.overrides =
                Some(IndexMap::from([("bar".to_string(), "file:../bar".to_string())]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("override")),
        "decision was {decision:?}",
    );
}

/// Registry and `link:` overrides keep the fast path: neither redirects
/// a dependency to contents only a refetch would pick up.
#[test]
fn returns_up_to_date_when_overrides_are_not_local_paths() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([
                ("bar".to_string(), "^2.0.0".to_string()),
                ("baz".to_string(), "link:../baz".to_string()),
            ]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// A `packageExtensions` entry injecting a local file dependency must
/// bail: extensions are merged into matching packages' manifests during
/// the full install, so the spec never appears in a project manifest.
#[test]
fn returns_skipped_when_a_package_extension_injects_a_local_file_dependency() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.package_extensions = Some(IndexMap::from([(
                "foo@1".to_string(),
                pnpm_config::PackageExtension {
                    dependencies: Some(BTreeMap::from([(
                        "bar".to_string(),
                        "file:../bar".to_string(),
                    )])),
                    ..Default::default()
                },
            )]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("package extension")),
        "decision was {decision:?}",
    );
}

/// A local file dependency injected via a packageExtension's
/// optionalDependencies does not bail when optionals are excluded from
/// the install — they aren't installed, so their contents can't be stale.
#[test]
fn returns_up_to_date_when_a_package_extension_optional_dependency_is_excluded() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.package_extensions = Some(IndexMap::from([(
                "foo@1".to_string(),
                pnpm_config::PackageExtension {
                    optional_dependencies: Some(BTreeMap::from([(
                        "bar".to_string(),
                        "file:../bar".to_string(),
                    )])),
                    ..Default::default()
                },
            )]));
        },
    );

    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: false,
    };
    let settings = current_settings(config, pnpm_config::NodeLinker::Isolated, included, None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_existing_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included,
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
    });
    assert_eq!(decision, Decision::UpToDate);
}

/// An unparsable `pnpm.overrides` (here a `catalog:` reference with no
/// matching catalog entry) bails to the full install with the
/// parse-error reason, not the local-file reason: the cause is a
/// misconfiguration, and attributing it to a local file dependency
/// would mislead troubleshooting.
#[test]
fn returns_skipped_with_parse_error_reason_when_overrides_cannot_be_parsed() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([("bar".to_string(), "catalog:".to_string())]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("cannot be parsed")),
        "decision was {decision:?}",
    );
}

/// A `catalog:` dependency whose catalog entry holds a bare local path
/// is a local file dependency after dereferencing — the catalog
/// resolver only bans the `workspace:`, `link:`, and `file:` protocols,
/// so the bare-path spelling reaches the local resolver. Same bail as
/// a direct local path.
#[test]
fn returns_skipped_when_a_catalog_dependency_resolves_to_a_local_path() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"catalog:"}"#,
    );

    let catalogs: Catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("foo".to_string(), "../foo".to_string())]),
    )]);
    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &catalogs,
    });
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
        "decision was {decision:?}",
    );
}

/// A `catalog:` dependency resolving to a registry range keeps the
/// fast path: the dereferenced specifier is not a local path.
#[test]
fn returns_up_to_date_when_a_catalog_dependency_resolves_to_a_registry_range() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"catalog:"}"#,
    );

    let catalogs: Catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("foo".to_string(), "^1.0.0".to_string())]),
    )]);
    // Record the catalogs in the state so the catalog-cache comparison
    // passes and the spec-deref path is what gets exercised, not the
    // "catalogs cache outdated" bail.
    let settings = current_settings_with_catalogs(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
        &catalogs,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_existing_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &catalogs,
    });
    assert_eq!(decision, Decision::UpToDate);
}

/// A `pnpm.overrides` entry spelled `catalog:` whose catalog entry
/// holds a local path bails like a direct local file override —
/// overrides are dereferenced through `parse_config_overrides` before
/// the check.
#[test]
fn returns_skipped_when_an_override_maps_through_a_catalog_to_a_local_path() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([("bar".to_string(), "catalog:".to_string())]));
        },
    );

    let catalogs: Catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("bar".to_string(), "./vendor/bar".to_string())]),
    )]);
    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &catalogs,
    });
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("override")),
        "decision was {decision:?}",
    );
}

/// Specs the git, remote-tarball, and registry resolvers claim must not
/// bail — matching them would disable the fast path for every project
/// with git dependencies.
#[test]
fn returns_up_to_date_when_specs_are_not_local_paths() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        concat!(
            r#""dependencies":{"foo":"user/repo","bar":"github:user/repo","#,
            // `quux` is a git shorthand whose committish ends in .tgz — it
            // must not be mistaken for a local tarball.
            r#""baz":"https://example.com/pkg.tgz","qux":"~1.2.3","quux":"user/repo#release.tgz"}"#,
        ),
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// `link:` dependencies are symlinked — changes inside them flow
/// through without a reinstall, so they don't invalidate the fast path.
#[test]
fn returns_up_to_date_when_a_project_has_only_link_dependencies() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"link:../foo"}"#,
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
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
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("disabled")));
}

/// No `.pnpm-workspace-state-v1.json` on disk → cannot prove
/// freshness.
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
        pnpm_config::NodeLinker::Isolated,
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
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");

    // Touch the manifest after the workspace-state was stamped.
    let manifest_path = dir.path().join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let refreshed_manifest = PackageManifest::from_path(manifest_path).unwrap();

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
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
        setup_fresh_install(pnpm_config::NodeLinker::Hoisted, "root", "1.0.0", "");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Project list mismatch (cached state has a project that today's
/// walk doesn't) invalidates the cached state.
#[test]
fn returns_skipped_when_workspace_project_set_changes() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");

    // Append a fake second-project entry to the cached state so
    // count + identity diverge from today's single-project walk.
    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        dir.path().join("pkg-a").to_string_lossy().into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some("1.0.0".into()) },
    );
    // Re-stamp so every file reads as validated and the mtime branch
    // cannot fire. This test is about the project-list branch.
    write_state(dir.path(), backdate_existing_files(dir.path()), settings, projects);

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("project list")));
}

/// Drift in `overrides` invalidates the cached state.
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
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `injectWorkspacePackages` invalidates the cached state.
/// Toggling the flag changes whether workspace resolutions land as
/// `link:` symlinks or `file:` hard-linked copies, so the previous
/// install's virtual store no longer matches what a fresh resolution
/// would produce. The assertion lives here so the wiring stays in
/// place.
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `enableGlobalVirtualStore` invalidates the cached state.
/// Toggling it moves the virtual store between `<storeDir>/links` and
/// each project's `node_modules/.pnpm`, so the previous install's
/// layout no longer matches a fresh resolution. The toggle is invisible
/// to the freshness check unless the key joins the comparison.
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
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
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");

    let mut settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    settings.enable_global_virtual_store = Some(false);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_existing_files(dir.path()), settings, projects);

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `ignoredOptionalDependencies` invalidates the cached
/// state.
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `patchedDependencies` invalidates the cached state.
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
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
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    // Validate everything on disk, then bump the patch past that timestamp.
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);
    fs::write(&patch_path, "--- a\n+++ b\n+edited\n").unwrap();

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
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
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    // Both the manifest and patch were written before this timestamp.
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// Drift in `dedupePeers` invalidates the cached state — the condition
/// the optimistic-repeat-install gate checks here.
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `preferWorkspacePackages` invalidates the cached state —
/// the condition the optimistic-repeat-install gate checks here.
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `peersSuffixMaxLength` invalidates the cached state.
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `packageExtensions` invalidates the cached state.
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
        pnpm_config::PackageExtension { dependencies: Some(deps), ..Default::default() };
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
        pnpm_config::PackageExtension { dependencies: Some(deps), ..Default::default() },
    );
    stale_config.package_extensions = Some(extensions);
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// Drift in `allowBuilds` invalidates the cached state.
#[test]
fn returns_skipped_when_allow_builds_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.allow_builds.insert("foo".to_string(), true);
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.allow_builds.insert("foo".to_string(), false);
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));

    let decision = check_optimistic_repeat_install_ignoring(
        &OptimisticRepeatInstallCheck {
            workspace_root,
            config,
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
            project_manifests: &[(workspace_root.to_path_buf(), &manifest)],
            is_workspace_install: false,
            lockfile: MaybeLazyLockfile::Loaded(None),
            catalogs: &BTreeMap::default(),
        },
        &["allowBuilds"],
    );
    assert_eq!(decision, Decision::UpToDate);
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
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// `explicit_settings` stands in for pnpm's raw (default-`undefined`)
/// config value, which is what gates whether the policy is recorded.
#[test]
fn returns_skipped_when_trust_policy_is_newly_configured() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let mut stale_config = Config::new();
    stale_config.modules_dir = workspace_root.join("node_modules");
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    assert_eq!(stale_settings.trust_policy, None, "an unconfigured policy is not recorded");
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.trust_policy = pnpm_config::TrustPolicy::NoDowngrade;
    config
        .explicit_settings
        .insert("trustPolicy".to_string(), serde_json::Value::String("no-downgrade".to_string()));
    let config = config.leak();

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}

/// See `WorkspaceStateSettings::minimum_release_age_strict` for the
/// resolution rule being mirrored.
#[test]
fn records_minimum_release_age_strict_like_pnpm_resolves_it() {
    let mut config = Config::new();
    config.explicit_settings.insert("minimumReleaseAge".to_string(), serde_json::Value::from(1440));
    let settings =
        current_settings(&config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    assert_eq!(settings.minimum_release_age_strict, Some(true));

    config.minimum_release_age_strict = Some(false);
    let settings =
        current_settings(&config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    assert_eq!(settings.minimum_release_age_strict, Some(false), "an explicit value wins");
}

/// State written by pnpm with a field pacquet doesn't read or
/// consume during install (e.g. `packageExtensions`,
/// `excludeLinksFromLockfile`) does NOT trip the settings-drift gate.
/// Pacquet ignores those fields because its install pipeline
/// doesn't react to them — invalidating the fast path on a value
/// pacquet can't actually consume would force a redundant reinstall
/// every time a user runs `pacquet install` after `pnpm install` in
/// the same project, which is the scenario the vlt benchmark
/// exercises.
///
/// As each setting is ported end-to-end (yaml plumbing, `Config`
/// field, real consumer, and joined into `current_settings`), it
/// joins [`settings_match`]'s comparison automatically and a
/// drift on it starts rejecting again.
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
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    // Populate fields pacquet records but `settings_match` does not
    // compare, to prove a difference on them keeps the fast path.
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
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

#[test]
fn returns_outdated_when_workspace_catalog_cache_changes() {
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

    let recorded_catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^18.0.0".to_string())]),
    )]);
    let settings = current_settings_with_catalogs(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
        &recorded_catalogs,
    );

    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    let current_catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^19.0.0".to_string())]),
    )]);
    let decision = check_workspace(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
        &current_catalogs,
    );
    assert_eq!(decision, Decision::Skipped { reason: "catalogs cache outdated" });
}

#[test]
fn returns_outdated_when_single_project_catalog_cache_changes() {
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

    let recorded_catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^18.0.0".to_string())]),
    )]);
    let settings = current_settings_with_catalogs(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
        &recorded_catalogs,
    );

    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    let current_catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^19.0.0".to_string())]),
    )]);
    let decision = check_with_catalogs(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
        false,
        &current_catalogs,
    );
    assert_eq!(decision, Decision::Skipped { reason: "catalogs cache outdated" });
}

/// `allowBuilds` is the one field where pnpm and pacquet round-trip
/// an empty configured value differently: pnpm writes `Some({})` for
/// an empty allow-list, while pacquet's [`current_settings`] writes
/// `None`. The comparison must treat the two as equivalent —
/// otherwise the cross-package-manager scenario rejects the fast path
/// on every iteration where pnpm wrote the state. The read side
/// coerces an absent value to an empty map to match.
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
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    // Simulate a pnpm-written state: empty `allowBuilds` map
    // serialized as `{}`, where pacquet would have written `None`.
    settings.allow_builds = Some(BTreeMap::new());

    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// Workspace install where a sibling project declares dependencies
/// but its `node_modules` is missing → not up to date.
///
/// The check only matters for sibling projects: the root's state
/// file lives inside `<workspace_root>/node_modules`, so a missing
/// root `node_modules` already trips the earlier "no workspace
/// state" guard.
#[test]
fn returns_skipped_when_sibling_node_modules_missing_for_project_with_deps() {
    let (dir, config, root_manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");

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
    // project-structure check passes, and backdate the tree so the mtime
    // branch reads as validated. We want the modules-dir branch to be the
    // deciding factor.
    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        sibling_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_existing_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests: &[
            (dir.path().to_path_buf(), &root_manifest),
            (sibling_dir, &sibling_manifest),
        ],
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
    });
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("node_modules")));
}

/// Regression: a single-project install with `node_modules` present
/// but no `pnpm-lock.yaml` on disk must NOT short-circuit. The
/// single-project branch raises `RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND`,
/// which resolves to not-up-to-date. Without this gate, pacquet's fast
/// path fires whenever the workspace-state file and manifests agree —
/// independent of whether the lockfile exists — which silently turns
/// the `cache+node_modules` and `node_modules`-only benchmark
/// scenarios into a 35 ms no-op.
#[test]
fn returns_skipped_when_lockfile_missing_in_single_project_mode() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");

    // `setup_fresh_install` seeds `pnpm-lock.yaml` for happy-path
    // tests; delete it here so this test exercises the missing-
    // lockfile branch.
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).expect("remove seeded lockfile");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("wanted lockfile")),
        "expected Skipped(wanted lockfile missing), got {decision:?}",
    );
}

/// Workspace installs do NOT require `pnpm-lock.yaml` on disk for
/// the fast path — the workspace branch reports up to date purely off
/// the per-manifest mtime check without any wanted-lockfile probe (its
/// merge-conflict scan silently `continue`s on ENOENT). Pacquet must
/// match that polarity so a workspace install state file written by
/// either tool round-trips through the other.
#[test]
fn returns_up_to_date_in_workspace_mode_without_lockfile() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");

    // Same seeded state as the happy path, but the lockfile gets
    // wiped first — the workspace branch shouldn't care.
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).expect("remove seeded lockfile");

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
    });
    assert_eq!(decision, Decision::UpToDate);
}

#[test]
fn returns_skipped_when_wanted_lockfile_has_merge_conflict_markers() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");
    fs::write(
        dir.path().join(Lockfile::FILE_NAME),
        "<<<<<<< ours\nlockfileVersion: '9.0'\n=======\nlockfileVersion: '10.0'\n>>>>>>> theirs\n",
    )
    .expect("write conflicted lockfile");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );

    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("conflict")));
}

#[test]
fn run_status_reports_wanted_lockfile_merge_conflicts() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");
    fs::write(
        dir.path().join(Lockfile::FILE_NAME),
        "<<<<<<< ours\nlockfileVersion: '9.0'\n=======\nlockfileVersion: '10.0'\n>>>>>>> theirs\n",
    )
    .expect("write conflicted lockfile");
    let state = load_workspace_state(dir.path()).expect("load workspace state").unwrap();

    let status = check_deps_status_before_run(
        &OptimisticRepeatInstallCheck {
            workspace_root: dir.path(),
            config,
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
            project_manifests: &[(dir.path().to_path_buf(), &manifest)],
            is_workspace_install: false,
            lockfile: MaybeLazyLockfile::Loaded(None),
            catalogs: &BTreeMap::default(),
        },
        &state,
    );

    assert!(matches!(
        status,
        RunDepsStatus::Outdated { issue, .. }
            if issue == format!("The lockfile in {} has merge conflicts", dir.path().display()),
    ),);
}

#[test]
fn returns_skipped_when_project_lockfile_has_merge_conflict_markers() {
    let (dir, config, root_manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        "",
        |config| config.shared_workspace_lockfile = false,
    );
    let project_root = dir.path().join("packages/project");
    fs::create_dir_all(&project_root).expect("create project");
    fs::write(project_root.join("package.json"), r#"{"name":"project","version":"1.0.0"}"#)
        .expect("write project manifest");
    fs::write(
        project_root.join(Lockfile::FILE_NAME),
        "<<<<<<< ours\nlockfileVersion: '9.0'\n=======\nlockfileVersion: '10.0'\n>>>>>>> theirs\n",
    )
    .expect("write project lockfile");
    let project_manifest =
        PackageManifest::from_path(project_root.join("package.json")).expect("read manifest");

    let decision = check_workspace(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &root_manifest), (project_root, &project_manifest)],
        &BTreeMap::default(),
    );

    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("conflict")));
}

#[test]
fn returns_skipped_when_lockfile_is_not_a_regular_file() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");
    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    fs::remove_file(&lockfile_path).expect("remove lockfile");
    fs::create_dir(&lockfile_path).expect("replace lockfile with directory");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );

    dbg!(&decision);
    assert!(matches!(
        decision,
        Decision::Skipped { reason } if reason.contains("cannot be checked")
    ));
}

#[cfg(unix)]
#[test]
fn returns_skipped_without_following_a_lockfile_symlink() {
    use std::os::unix::fs::symlink;

    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");
    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    fs::remove_file(&lockfile_path).expect("remove lockfile");
    symlink("/dev/zero", &lockfile_path).expect("replace lockfile with symlink");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );

    dbg!(&decision);
    assert!(matches!(
        decision,
        Decision::Skipped { reason } if reason.contains("cannot be checked")
    ));
}

#[test]
fn returns_skipped_without_scanning_an_oversized_changed_lockfile() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");
    let lockfile = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(dir.path().join(Lockfile::FILE_NAME))
        .expect("open lockfile");
    lockfile.set_len(MAX_LOCKFILE_CONFLICT_SCAN_BYTES).expect("resize lockfile");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );

    dbg!(&decision);
    assert!(matches!(
        decision,
        Decision::Skipped { reason } if reason.contains("cannot be checked")
    ));
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

#[test]
fn returns_skipped_when_current_lockfile_missing_for_non_empty_wanted_lockfile() {
    let (dir, config) = setup_content_check_project();
    fs::remove_file(config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME)).unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("current lockfile")),
        "expected Skipped(current lockfile missing), got {decision:?}",
    );
}

#[test]
fn returns_skipped_when_current_lockfile_missing_for_wanted_lockfile_with_importer_deps() {
    let (dir, config) = setup_content_check_project();
    let workspace_root = dir.path();
    fs::write(workspace_root.join(Lockfile::FILE_NAME), FOO_LOCKFILE_WITHOUT_PACKAGES).unwrap();

    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    fs::remove_file(config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME)).unwrap();
    let manifest = PackageManifest::from_path(workspace_root.join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(workspace_root.to_path_buf(), &manifest)]);
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("current lockfile")),
        "expected Skipped(current lockfile missing), got {decision:?}",
    );
}

#[test]
fn returns_skipped_when_current_lockfile_is_empty_for_non_empty_wanted_lockfile() {
    let (dir, config) = setup_content_check_project();
    fs::write(config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME), "").unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("current lockfile")),
        "expected Skipped(current lockfile missing), got {decision:?}",
    );
}

/// A manifest rewrite that leaves the dependency fields intact — the
/// shape `touch package.json` / `npm pkg set/delete` produce — must
/// still short-circuit because the wanted lockfile remains up to date.
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
/// modules directory no longer reflects it — the
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

/// Only the wanted lockfile changed (a `git checkout` / stash-restore of
/// just `pnpm-lock.yaml`), with every manifest left untouched. The
/// manifest-mtime fast path must not skip the lockfile change.
#[test]
fn returns_skipped_when_only_the_lockfile_changed() {
    let (dir, config) = setup_content_check_project();

    // Rewrite only the wanted lockfile; package.json keeps its original
    // (pre-state) mtime, so `modifiedProjects` is empty.
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
/// path.
#[test]
fn workspace_content_check_refreshes_last_validated_timestamp() {
    let (dir, config) = setup_content_check_project();
    let before = pnpm_workspace_state::load_workspace_state(dir.path())
        .unwrap()
        .unwrap()
        .last_validated_timestamp;

    fs::write(dir.path().join("package.json"), FOO_MANIFEST).unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, true, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(decision, Decision::UpToDate);

    let after = pnpm_workspace_state::load_workspace_state(dir.path())
        .unwrap()
        .unwrap()
        .last_validated_timestamp;
    assert!(after > before, "expected the state timestamp to advance ({before} -> {after})");
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

#[test]
fn workspace_content_check_converges_after_a_same_millisecond_mtime_collision() {
    assert_content_check_converges_after_collision(500_000);
}

#[test]
fn workspace_content_check_converges_after_a_whole_second_mtime_collision() {
    assert_content_check_converges_after_collision(0);
}

/// The refreshed baseline must not post-date the filesystem clock: an
/// edit made after the content check passed still has to defeat the
/// mtime fast path on the next run.
#[test]
fn workspace_content_check_does_not_bless_a_manifest_edited_after_it_passed() {
    let (dir, config) = setup_content_check_project();
    collide_mtimes_with_recorded_state(dir.path(), config, 500_000);
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
    assert_eq!(
        content_check_decision(&dir, config, true, &[(dir.path().to_path_buf(), &manifest)]),
        Decision::UpToDate,
    );

    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"root","version":"1.0.0","dependencies":{"foo":"^1.0.0","bar":"^1.0.0"}}"#,
    )
    .unwrap();
    let edited = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, true, &[(dir.path().to_path_buf(), &edited)]);
    assert!(
        matches!(decision, Decision::Skipped { .. }),
        "expected the edit to defeat the fast path, got {decision:?}",
    );
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

#[test]
fn run_gate_converges_after_a_same_millisecond_mtime_collision() {
    assert_deps_status_converges_after_collision(500_000);
}

#[test]
fn run_gate_converges_after_a_whole_second_mtime_collision() {
    assert_deps_status_converges_after_collision(0);
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

#[test]
fn returns_up_to_date_when_aliased_workspace_dependency_satisfies_range() {
    assert_eq!(
        linked_sibling_decision_for_spec(
            "alias",
            "npm:pkg-a@^1.0.0",
            "link:pkg-a",
            "1.5.0",
            pnpm_config::LinkWorkspacePackages::DirectOnly,
        ),
        Decision::UpToDate,
    );
}

#[test]
fn returns_skipped_when_aliased_workspace_dependency_version_is_outdated() {
    let decision = linked_sibling_decision_for_spec(
        "alias",
        "npm:pkg-a@^1.0.0",
        "link:pkg-a",
        "2.0.0",
        pnpm_config::LinkWorkspacePackages::DirectOnly,
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("linked")),
        "expected Skipped(linked package out of date), got {decision:?}",
    );
}

#[test]
fn returns_up_to_date_when_linked_workspace_dependency_uses_a_tag() {
    assert_eq!(
        linked_sibling_decision_for_spec(
            "pkg-a",
            "unpublished-tag",
            "link:pkg-a",
            "1.0.0",
            pnpm_config::LinkWorkspacePackages::DirectOnly,
        ),
        Decision::UpToDate,
    );
}

#[test]
fn returns_up_to_date_for_registry_resolution_when_workspace_linking_is_off() {
    assert_eq!(
        linked_sibling_decision_for_spec(
            "pkg-a",
            "1.0.0",
            "1.0.0",
            "1.0.0",
            pnpm_config::LinkWorkspacePackages::Off,
        ),
        Decision::UpToDate,
    );
}

#[test]
fn injected_self_reference_resolved_as_link_is_up_to_date() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let sibling_dir = workspace_root.join("pkg-a");
    fs::create_dir_all(&sibling_dir).unwrap();
    fs::write(
        workspace_root.join("package.json"),
        r#"{"name":"root","version":"1.0.0","dependencies":{"pkg-a":"file:pkg-a"}}"#,
    )
    .unwrap();
    fs::write(sibling_dir.join("package.json"), r#"{"name":"pkg-a","version":"1.0.0"}"#).unwrap();
    let root_manifest = PackageManifest::from_path(workspace_root.join("package.json")).unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_dir.join("package.json")).unwrap();
    let lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      pkg-a:
        specifier: file:pkg-a
        version: link:pkg-a

  pkg-a: {}
",
    )
    .unwrap();
    let config = Config::new();
    let project_manifests =
        [(workspace_root.to_path_buf(), &root_manifest), (sibling_dir, &sibling_manifest)];
    let context = LinkedPackagesContext::new(&config, &project_manifests);

    assert!(linked_packages_are_up_to_date(
        &context,
        workspace_root,
        &root_manifest,
        lockfile.root_project().unwrap(),
    ));
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
    let before = pnpm_workspace_state::load_workspace_state(dir.path())
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
    let after = pnpm_workspace_state::load_workspace_state(dir.path())
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

/// The subject is compared at nanosecond precision against the
/// millisecond-precise reference, and a whole-second mtime counts its
/// entire second as possibly-after.
#[test]
fn modified_at_or_after_compares_at_nanosecond_precision() {
    let ms = 1_700_000_000_000_i64; // a whole millisecond, in ms
    let ns = ms * 1_000_000; // the same instant, in ns

    // Whole-second (coarse filesystem) mtime: the whole second is possibly-after.
    let coarse = FileMtime { ms, ns, whole_second: true };
    assert!(modified_at_or_after(coarse, ms));
    assert!(modified_at_or_after(coarse, ms + 999));
    // The reference is in a later second: the whole second is before it.
    assert!(!modified_at_or_after(coarse, ms + 1_000));

    // Sub-second mtime exactly on the millisecond boundary: equal is not after.
    let on_boundary = FileMtime { ms, ns, whole_second: false };
    assert!(!modified_at_or_after(on_boundary, ms));
    assert!(!modified_at_or_after(on_boundary, ms + 1));
    assert!(modified_at_or_after(on_boundary, ms - 1));

    // Same millisecond as the reference, half a millisecond later: the
    // millisecond values tie, but the nanosecond mtime does not, so the
    // edit is still seen (the same-millisecond flake this guards against).
    let later_in_same_ms = FileMtime { ms, ns: ns + 500_000, whole_second: false };
    assert!(modified_at_or_after(later_in_same_ms, ms));
}

/// On a sub-second filesystem the lockfile freshness check uses
/// whole-millisecond precision so the unchanged lockfile is never flagged
/// against its own millisecond-truncated baseline (which the nanosecond
/// manifest comparison would), while an external edit a millisecond later
/// is still caught. On a whole-second-mtime filesystem the whole second is
/// possibly-after, so a same-second external edit falls through to the
/// content check.
#[test]
fn lockfile_check_does_not_self_flag_its_own_baseline() {
    let ms = 1_700_000_000_000_i64;
    let subsecond_ns = ms * 1_000_000 + 500_000; // .5 ms into its millisecond
    let fine = FileMtime { ms, ns: subsecond_ns, whole_second: false };

    // A manifest with this mtime would (correctly) be flagged via
    // nanoseconds against a baseline equal to its own truncated ms:
    assert!(modified_at_or_after(fine, ms));
    // The lockfile must NOT self-flag against that same baseline:
    assert!(!lockfile_modified_since(fine, ms));
    // An external edit a whole millisecond later is still caught:
    assert!(lockfile_modified_since(
        FileMtime { ms: ms + 1, ns: subsecond_ns, whole_second: false },
        ms
    ));

    // Whole-second (coarse) filesystem: the whole second is possibly-after
    // its own baseline, so a same-second external edit is not missed.
    let coarse = FileMtime { ms, ns: ms * 1_000_000, whole_second: true };
    assert!(lockfile_modified_since(coarse, ms));
    // A whole second entirely before the baseline is not flagged.
    assert!(!lockfile_modified_since(coarse, ms + 1_000));
}

/// The reproduction command spells the dependency-group flags the way
/// the CLI accepts them
/// ([pnpm/pnpm#14147](https://github.com/pnpm/pnpm/issues/14147)). The
/// table mirrors pnpm's `createInstallArgs` test.
#[test]
fn install_args_reproduce_the_recorded_dependency_groups() {
    let args = |dev: Option<bool>, optional: Option<bool>, production: Option<bool>| {
        let state = WorkspaceState {
            settings: WorkspaceStateSettings { dev, optional, production, ..Default::default() },
            ..Default::default()
        };
        install_args_from_state(&state)
    };

    assert_eq!(args(None, Some(true), Some(true)), ["--prod"]);
    assert_eq!(args(None, Some(false), Some(true)), ["--prod", "--no-optional"]);
    assert_eq!(args(Some(true), Some(true), None), ["--dev"]);
    assert_eq!(args(Some(true), Some(false), None), ["--dev", "--no-optional"]);
    assert_eq!(args(Some(true), Some(true), Some(true)), [] as [&str; 0]);
    assert_eq!(args(Some(true), Some(false), Some(true)), ["--no-optional"]);
}
