use super::{
    super::{
        Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install,
        settings::current_settings,
    },
    FOO_MANIFEST, assert_content_check_converges_after_collision, check, check_with_lockfile,
    collide_mtimes_with_recorded_state, content_check_decision, isolated_included,
    linked_sibling_decision_for_spec, setup_content_check_project, setup_fresh_install,
    validate_existing_files, write_local_tarball_lockfile, write_state,
};
use pnpm_config::Config;
use pnpm_lockfile::MaybeLazyLockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::{backdate_existing_files, set_mtime};
use pnpm_workspace_state::{ProjectEntry, load_workspace_state, update_workspace_state};
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

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
