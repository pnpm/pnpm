use super::{
    super::{
        Decision, deps_status::RunDepsStatus, first_setting_drift, settings::current_settings,
    },
    MTIME_STEP_MS, backdate_validated_files, check, isolated_included, recorded_timestamp,
    single_project_deps_status, workspace_deps_status, write_empty_lockfile, write_state,
};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::set_mtime_ms;
use pnpm_workspace_state::{ProjectEntry, WorkspaceState};
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

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
        ProjectEntry {
            name: Some("root".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    write_state(workspace_root, backdate_validated_files(workspace_root), stale_settings, projects);

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
    config.workspace_dir = Some(workspace_root.to_path_buf());
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
        ProjectEntry {
            name: Some("root".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    // Validate everything on disk, then bump the patch past that timestamp.
    write_state(workspace_root, backdate_validated_files(workspace_root), settings, projects);
    fs::write(&patch_path, "--- a\n+++ b\n+edited\n").unwrap();

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("patch")));
}
/// With dedicated lockfiles a project's state lives in the project, and its
/// patch files are read from the workspace directory, as the config hashes
/// them.
#[test]
fn a_project_with_its_own_lockfile_sees_an_edited_workspace_patch() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();
    let manifest_path = project_dir.join("package.json");
    fs::write(&manifest_path, r#"{"name":"project","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(&project_dir);
    let patch_path = dir
        .path()
        .join("patches")
        .join("foo.patch");
    fs::create_dir_all(patch_path.parent().unwrap()).unwrap();
    fs::write(&patch_path, "--- a\n+++ b\n").unwrap();

    let mut config = Config::new();
    config.modules_dir = project_dir.join("node_modules");
    config.workspace_dir = Some(dir.path().to_path_buf());
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.patched_dependencies = Some(indexmap::IndexMap::from([(
        "foo@1.0.0".to_string(),
        "patches/foo.patch".to_string(),
    )]));
    let config = config.leak();
    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let projects = BTreeMap::from([(
        project_dir.to_string_lossy().into_owned(),
        ProjectEntry {
            name: Some("project".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    )]);
    write_state(&project_dir, backdate_validated_files(dir.path()), settings, projects);
    fs::write(&patch_path, "--- a\n+++ b\n+edited\n").unwrap();

    let decision = check(
        &project_dir,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(project_dir.clone(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("patch")));
}

/// pnpm v11 records each patch file resolved against the workspace
/// directory, `.` and `..` segments collapsed.
#[test]
fn a_patch_file_recorded_by_its_absolute_path_matches_its_configured_path() {
    let dir = tempdir().unwrap();
    let configured = |path: String| {
        let mut config = Config::new();
        config.workspace_dir = Some(dir.path().to_path_buf());
        config.patched_dependencies =
            Some(indexmap::IndexMap::from([("foo@1.0.0".to_string(), path)]));
        config
    };
    let config = configured("patches/foo.patch".to_string());
    let absolute = dir
        .path()
        .join("patches/foo.patch")
        .to_string_lossy()
        .into_owned();
    let state = WorkspaceState {
        settings: current_settings(
            &configured(absolute),
            pnpm_config::NodeLinker::Isolated,
            isolated_included(),
            None,
        ),
        ..Default::default()
    };
    let drift = |config: &Config| {
        first_setting_drift(
            &state,
            config,
            pnpm_config::NodeLinker::Isolated,
            isolated_included(),
            None,
            &[],
        )
    };
    assert_eq!(drift(&config), None);
    assert_eq!(drift(&configured("./shared/../patches/foo.patch".to_string())), None);
    assert_eq!(drift(&configured("patches/bar.patch".to_string())), Some("patchedDependencies"));
}

/// A newer patch mtime is resolved by comparing its content with the lockfile.
#[test]
fn workspace_compares_newer_patch_content_with_lockfile() {
    assert_newer_patch_content_compared_with_lockfile(workspace_deps_status);
}

/// [`workspace_compares_newer_patch_content_with_lockfile`] for a project
/// without a workspace manifest, whose unmodified manifest skips the project
/// content check.
#[test]
fn single_project_compares_newer_patch_content_with_lockfile() {
    assert_newer_patch_content_compared_with_lockfile(single_project_deps_status);
}

fn assert_newer_patch_content_compared_with_lockfile(
    deps_status: fn(
        &tempfile::TempDir,
        &'static Config,
        &[(std::path::PathBuf, &PackageManifest)],
    ) -> RunDepsStatus,
) {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    let patch_path = workspace_root.join("patches").join("foo.patch");
    fs::create_dir_all(patch_path.parent().unwrap()).unwrap();
    fs::write(&patch_path, "--- a\n+++ b\n").unwrap();

    let mut config = Config::new();
    config.workspace_dir = Some(workspace_root.to_path_buf());
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let mut patched = indexmap::IndexMap::new();
    patched.insert("foo@1.0.0".to_string(), "patches/foo.patch".to_string());
    config.patched_dependencies = Some(patched);
    let config = config.leak();
    let patch_hashes = config
        .patched_dependency_hashes()
        .unwrap()
        .unwrap();
    fs::write(
        workspace_root.join(Lockfile::FILE_NAME),
        format!(
            "lockfileVersion: '9.0'\npatchedDependencies:\n  foo@1.0.0: {}\nimporters:\n  .: {{}}\n",
            patch_hashes["foo@1.0.0"],
        ),
    )
    .unwrap();

    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry {
            name: Some("root".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    write_state(workspace_root, backdate_validated_files(workspace_root), settings, projects);
    let project_manifests = [(workspace_root.to_path_buf(), &manifest)];
    assert_eq!(
        check(workspace_root, config, pnpm_config::NodeLinker::Isolated, &project_manifests),
        Decision::UpToDate,
    );

    // Rewrite the patch without changing its content.
    fs::write(&patch_path, "--- a\n+++ b\n").unwrap();
    let rewritten_ms = recorded_timestamp(workspace_root) + MTIME_STEP_MS;
    set_mtime_ms(&patch_path, rewritten_ms);
    assert_eq!(deps_status(&dir, config, &project_manifests), RunDepsStatus::UpToDate);

    // An edit that keeps the recorded mtime is still an edit.
    fs::write(&patch_path, "--- a\n+++ b\n+unchecked\n").unwrap();
    set_mtime_ms(&patch_path, rewritten_ms);
    assert!(matches!(
        deps_status(&dir, config, &project_manifests),
        RunDepsStatus::Outdated { issue, .. } if issue == "Patches were modified"
    ));

    fs::write(&patch_path, "--- a\n+++ b\n+edited\n").unwrap();
    assert!(matches!(
        deps_status(&dir, config, &project_manifests),
        RunDepsStatus::Outdated { issue, .. } if issue == "Patches were modified"
    ));
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
        ProjectEntry {
            name: Some("root".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    write_state(workspace_root, backdate_validated_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
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
        ProjectEntry {
            name: Some("root".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    write_state(workspace_root, backdate_validated_files(workspace_root), settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}
/// Drift in `sideEffectsCache.exclude` invalidates the cached state: it
/// decides which global-virtual-store slots are scoped to the project.
#[test]
fn returns_skipped_when_side_effects_cache_exclude_drifts() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.side_effects_cache_exclude = Some(vec!["java".to_string()]);
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry {
            name: Some("root".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    write_state(workspace_root, backdate_validated_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}
