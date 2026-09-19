#[cfg(unix)]
use super::FOO_LOCKFILE;
use super::{
    super::{
        Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install,
        settings::current_settings,
    },
    FOO_MANIFEST, RunDepsStatus, assert_content_check_converges_after_collision,
    backdate_validated_files, check, check_with_lockfile, collide_mtimes_with_recorded_state,
    content_check_decision, isolated_included, linked_sibling_decision_for_spec,
    setup_content_check_project, setup_fresh_install, setup_fresh_install_with_config,
    validate_existing_files, workspace_deps_status, write_local_tarball_lockfile, write_state,
};
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::set_mtime;
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
    let modified = fs::metadata(&tarball)
        .expect("stat tarball")
        .modified()
        .expect("tarball mtime");
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
        dir.path()
            .to_string_lossy()
            .into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        dir.path()
            .join("pkg-a")
            .to_string_lossy()
            .into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some("1.0.0".into()) },
    );
    // Re-stamp so every file reads as validated and the mtime branch
    // cannot fire. This test is about the project-list branch.
    write_state(dir.path(), backdate_validated_files(dir.path()), settings, projects);

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("project list")));
}
/// Record the only project under a sibling of `workspace_root`, as a tree
/// copied from there carries it. Returns the recorded dir.
fn record_projects_elsewhere(workspace_root: &std::path::Path) -> String {
    let mut state =
        load_workspace_state(workspace_root).expect("read state").expect("state on disk");
    let (_, entry) = state.projects.pop_first().expect("the recorded project");
    let elsewhere = workspace_root
        .with_file_name("elsewhere")
        .to_string_lossy()
        .into_owned();
    state.projects.insert(elsewhere.clone(), entry);
    update_workspace_state(workspace_root, &state).expect("write workspace state");
    elsewhere
}
/// The patch mtimes of a moved tree come from wherever it was validated, so
/// a newer patch file leaves the verdict to the proof of the move, at the
/// fast path and at the gate alike. This tree has no `.modules.yaml` for
/// that proof to get past, so both refuse it for the move it cannot prove.
#[test]
fn a_moved_tree_leaves_a_newer_patch_to_the_move_proof() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        "",
        |config| {
            let patch = ("foo@1.0.0".to_string(), "patches/foo.patch".to_string());
            config.patched_dependencies = Some(indexmap::IndexMap::from([patch]));
        },
    );
    let elsewhere = record_projects_elsewhere(dir.path());
    fs::create_dir_all(dir.path().join("patches")).unwrap();
    fs::write(dir.path().join("patches/foo.patch"), "--- a\n+++ b\n").unwrap();
    let projects = [(dir.path().to_path_buf(), &manifest)];

    let decision = check(dir.path(), config, pnpm_config::NodeLinker::Isolated, &projects);
    assert_eq!(
        decision,
        Decision::Skipped {
            reason: "the moved tree's store or virtual store does not resolve from where it is",
        },
    );
    let status = workspace_deps_status(&dir, config, &projects);
    assert!(
        matches!(&status, RunDepsStatus::Outdated { issue, .. } if issue == "The workspace structure has changed since last install"),
        "unexpected status: {status:?}",
    );
    let state = load_workspace_state(dir.path()).expect("read state").expect("state on disk");
    assert!(state.projects.contains_key(&elsewhere), "the refused move must not re-key the state");
}
/// The current lockfile keeps only what the importers reach, so a wanted
/// lockfile still carrying a snapshot no importer reaches never equals it.
/// The proof of a move compares the shape a materialization would settle on,
/// as the in-place short-circuit does, so such a tree is reused.
#[cfg(unix)]
#[test]
fn a_moved_tree_with_unreachable_lockfile_snapshots_is_up_to_date() {
    let (dir, config) = setup_content_check_project();
    write_relocatable_layout(config);
    install_foo_slot(config);
    // A snapshot the only importer does not reach, as a lockfile keeps until
    // something prunes it.
    let wanted = format!("{FOO_LOCKFILE}  bar@1.0.0: {{}}\n");
    fs::write(dir.path().join(Lockfile::FILE_NAME), wanted).unwrap();
    record_projects_elsewhere(dir.path());
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(decision, Decision::UpToDate);
}
#[cfg(unix)]
fn write_relocatable_layout(config: &Config) {
    let layout = serde_json::json!({
        "layoutVersion": 5,
        "nodeLinker": "isolated",
        "included": isolated_included(),
        "hoistPattern": config.hoist_pattern,
        "publicHoistPattern": config.public_hoist_pattern,
        "storeDir": config.store_dir.display().to_string(),
        "virtualStoreDir": config.effective_virtual_store_dir().to_string_lossy(),
        "virtualStoreDirMaxLength": config.virtual_store_dir_max_length,
    });
    fs::write(config.modules_dir.join(pnpm_modules_yaml::MODULES_FILENAME), layout.to_string())
        .unwrap();
}

#[cfg(unix)]
fn install_foo_slot(config: &Config) {
    let slot = config.virtual_store_dir.join("foo@1.0.0/node_modules/foo");
    fs::create_dir_all(&slot).unwrap();
    std::os::unix::fs::symlink(".pnpm/foo@1.0.0/node_modules/foo", config.modules_dir.join("foo"))
        .unwrap();
}

#[cfg(unix)]
#[test]
fn a_moved_tree_with_a_missing_dependency_is_not_up_to_date() {
    for missing in ["link", "slot"] {
        let (dir, config) = setup_content_check_project();
        write_relocatable_layout(config);
        install_foo_slot(config);
        match missing {
            "link" => fs::remove_file(config.modules_dir.join("foo")).unwrap(),
            _ => fs::remove_dir_all(config.virtual_store_dir.join("foo@1.0.0")).unwrap(),
        }
        record_projects_elsewhere(dir.path());
        let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
        let projects = [(dir.path().to_path_buf(), &manifest)];
        let decision = content_check_decision(&dir, config, false, &projects);
        assert!(matches!(decision, Decision::Skipped { .. }), "missing {missing}: {decision:?}");
        let status = workspace_deps_status(&dir, config, &projects);
        assert!(matches!(status, RunDepsStatus::Outdated { .. }), "missing {missing}: {status:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_moved_tree_cannot_trust_a_pnpmfile_with_an_older_mtime() {
    let (dir, config) = setup_content_check_project();
    write_relocatable_layout(config);
    install_foo_slot(config);
    let elsewhere = record_projects_elsewhere(dir.path());
    let hook = dir.path().join(".pnpmfile.cjs");
    fs::write(&hook, "module.exports = { hooks: { readPackage(pkg) { pkg.dependencies = {}; return pkg; } } };\n")
        .unwrap();
    let mut state = load_workspace_state(dir.path()).unwrap().unwrap();
    state.pnpmfiles = vec![
        std::path::Path::new(&elsewhere)
            .join(".pnpmfile.cjs")
            .to_string_lossy()
            .into_owned(),
    ];
    update_workspace_state(dir.path(), &state).unwrap();
    pnpm_testing_utils::fs::set_mtime_ms(&hook, state.last_validated_timestamp - 2_000);
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
    let projects = [(dir.path().to_path_buf(), &manifest)];
    let decision = content_check_decision(&dir, config, false, &projects);
    assert!(matches!(decision, Decision::Skipped { .. }), "unexpected decision: {decision:?}");
    let status = workspace_deps_status(&dir, config, &projects);
    assert!(matches!(status, RunDepsStatus::Outdated { .. }), "unexpected status: {status:?}");
}

#[cfg(unix)]
#[test]
fn a_moved_production_install_passes_the_run_gate() {
    let (dir, config) = setup_content_check_project();
    write_relocatable_layout(config);
    let included = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: true,
    };
    let layout_path = config.modules_dir.join(pnpm_modules_yaml::MODULES_FILENAME);
    let mut layout: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&layout_path).unwrap()).unwrap();
    layout["included"] = serde_json::to_value(included).unwrap();
    fs::write(layout_path, layout.to_string()).unwrap();
    fs::write(
        dir.path().join("package.json"),
        FOO_MANIFEST.replace("dependencies", "devDependencies"),
    )
    .unwrap();
    fs::write(
        dir.path().join(Lockfile::FILE_NAME),
        FOO_LOCKFILE.replace("dependencies:", "devDependencies:"),
    )
    .unwrap();
    let wanted = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();
    let current =
        crate::filter_lockfile_for_current(&wanted, included, &crate::SkippedSnapshots::new());
    current
        .save_to_path(&config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME))
        .unwrap();
    record_projects_elsewhere(dir.path());
    let mut state = load_workspace_state(dir.path()).unwrap().unwrap();
    state.settings.dev = Some(false);
    update_workspace_state(dir.path(), &state).unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
    let status = workspace_deps_status(&dir, config, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(status, RunDepsStatus::UpToDate);
    assert_eq!(load_workspace_state(dir.path()).unwrap().unwrap().settings.dev, Some(false));
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
    write_state(workspace_root, backdate_validated_files(workspace_root), stale_settings, projects);

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
    write_state(workspace_root, backdate_validated_files(workspace_root), stale_settings, projects);

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
        dir.path()
            .to_string_lossy()
            .into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        sibling_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_validated_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: &[
            (dir.path().to_path_buf(), &root_manifest),
            (sibling_dir, &sibling_manifest),
        ],
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
        layout: crate::RepeatInstallLayout {
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifest_freshness: crate::ManifestFreshness::Mtime,
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
/// Every project the content check takes is checked, not only the first: a
/// project the lockfile no longer records is refused however current the root
/// is.
#[test]
fn workspace_content_check_refuses_a_project_after_the_first() {
    let (dir, config) = setup_content_check_project();
    let dropped_dir = dir.path().join("packages").join("b");
    fs::create_dir_all(&dropped_dir).unwrap();
    fs::write(dropped_dir.join("package.json"), r#"{"name":"b","version":"1.0.0"}"#).unwrap();
    // Rewritten so both manifests are newer than the recorded validation and
    // both reach the content check, the root's first.
    fs::write(dir.path().join("package.json"), FOO_MANIFEST).unwrap();
    let mut state = load_workspace_state(dir.path()).unwrap().unwrap();
    state.projects.insert(
        dropped_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("b".into()), version: Some("1.0.0".into()) },
    );
    update_workspace_state(dir.path(), &state).unwrap();
    let root = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
    let dropped = PackageManifest::from_path(dropped_dir.join("package.json")).unwrap();

    let decision = content_check_decision(
        &dir,
        config,
        true,
        &[(dir.path().to_path_buf(), &root), (dropped_dir, &dropped)],
    );

    assert_eq!(
        decision,
        Decision::Skipped { reason: "a modified manifest is no longer satisfied by the lockfile" },
    );
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
/// Under `dedupeDirectDeps` a sibling whose direct dependencies resolve to
/// the root's targets gets nothing linked and no modules directory, so the
/// missing directory is not evidence of a missing install.
#[test]
fn returns_up_to_date_when_a_deduped_sibling_has_no_node_modules() {
    assert_eq!(deduped_sibling_decision(true, "1.0.0", "1.0.0"), Decision::UpToDate);
}
#[test]
fn returns_skipped_when_a_sibling_without_dedupe_has_no_node_modules() {
    let decision = deduped_sibling_decision(false, "1.0.0", "1.0.0");
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("node_modules")));
}
/// The same specifier can resolve to another peer set for the sibling; the
/// linker then links it into the sibling, so its missing `node_modules` is
/// real damage.
#[test]
fn returns_skipped_when_a_sibling_resolves_a_shared_specifier_to_another_peer_set() {
    let decision = deduped_sibling_decision(true, "1.0.0", "1.0.0(bar@1.0.0)");
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("node_modules")));
}
/// `link:` targets are compared where they point, not as strings: the root's
/// `link:libs/lib` and the sibling's `link:../libs/lib` are one directory.
#[test]
fn returns_up_to_date_when_a_deduped_sibling_links_the_same_directory_by_another_path() {
    assert_eq!(
        deduped_sibling_decision(true, "link:libs/lib", "link:../libs/lib"),
        Decision::UpToDate,
    );
}
#[test]
fn returns_skipped_when_a_sibling_links_another_directory_under_the_same_specifier() {
    let decision = deduped_sibling_decision(true, "link:libs/lib", "link:libs/lib");
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("node_modules")));
}
/// The root declares `foo` in two groups with one target: still one target,
/// so the sibling matches it.
#[test]
fn returns_up_to_date_when_the_root_declares_the_alias_in_two_groups_with_one_target() {
    assert_eq!(
        deduped_sibling_decision_with_root_dev("1.0.0", Some("1.0.0"), "1.0.0"),
        Decision::UpToDate,
    );
}
/// Two root declarations with differing targets have one effective target
/// the linker picks by group order; the check does not reproduce that
/// choice and falls through.
#[test]
fn returns_skipped_when_the_root_declares_the_alias_with_differing_targets() {
    let decision = deduped_sibling_decision_with_root_dev("1.0.0", Some("2.0.0"), "2.0.0");
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("node_modules")));
}
/// A production-only install never links dev dependencies, so a sibling dev
/// dependency the root lacks does not make the sibling incomplete.
#[test]
fn returns_up_to_date_when_an_unmatched_dependency_is_in_an_excluded_group() {
    let production_only = IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };
    assert_eq!(
        deduped_sibling_decision_in(DedupedSibling {
            dedupe_direct_deps: true,
            root_version: "1.0.0",
            root_dev_version: None,
            sibling_version: "1.0.0",
            sibling_dev_bar_version: Some("2.0.0"),
            included: production_only,
        }),
        Decision::UpToDate,
    );
}
/// The same sibling dev dependency blocks the exemption once dev
/// dependencies are installed.
#[test]
fn returns_skipped_when_an_unmatched_dependency_is_in_an_included_group() {
    let decision = deduped_sibling_decision_in(DedupedSibling {
        dedupe_direct_deps: true,
        root_version: "1.0.0",
        root_dev_version: None,
        sibling_version: "1.0.0",
        sibling_dev_bar_version: Some("2.0.0"),
        included: isolated_included(),
    });
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("node_modules")));
}
fn deduped_sibling_decision(
    dedupe_direct_deps: bool,
    root_version: &str,
    sibling_version: &str,
) -> Decision {
    deduped_sibling_decision_in(DedupedSibling {
        dedupe_direct_deps,
        root_version,
        root_dev_version: None,
        sibling_version,
        sibling_dev_bar_version: None,
        included: isolated_included(),
    })
}
fn deduped_sibling_decision_with_root_dev(
    root_version: &str,
    root_dev_version: Option<&str>,
    sibling_version: &str,
) -> Decision {
    deduped_sibling_decision_in(DedupedSibling {
        dedupe_direct_deps: true,
        root_version,
        root_dev_version,
        sibling_version,
        sibling_dev_bar_version: None,
        included: isolated_included(),
    })
}
#[derive(Clone, Copy)]
struct DedupedSibling<'a> {
    dedupe_direct_deps: bool,
    /// The root's `dependencies.foo`.
    root_version: &'a str,
    /// The root's `devDependencies.foo`, when it declares one.
    root_dev_version: Option<&'a str>,
    /// The sibling's `devDependencies.foo`.
    sibling_version: &'a str,
    /// The sibling's `devDependencies.bar`, which the root never declares.
    sibling_dev_bar_version: Option<&'a str>,
    included: IncludedDependencies,
}
fn deduped_sibling_decision_in(sibling: DedupedSibling<'_>) -> Decision {
    let DedupedSibling {
        dedupe_direct_deps,
        root_version,
        root_dev_version,
        sibling_version,
        sibling_dev_bar_version,
        included,
    } = sibling;
    let (dir, config, root_manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"1.0.0"}"#,
        |config| config.dedupe_direct_deps = dedupe_direct_deps,
    );
    let sibling_dir = dir.path().join("pkg-a");
    fs::create_dir_all(&sibling_dir).unwrap();
    let sibling_manifest_path = sibling_dir.join("package.json");
    let sibling_bar_manifest = sibling_dev_bar_version.map_or("", |_| r#","bar":"2.0.0""#);
    fs::write(
        &sibling_manifest_path,
        format!(
            r#"{{"name":"pkg-a","version":"1.0.0","devDependencies":{{"foo":"1.0.0"{sibling_bar_manifest}}}}}"#,
        ),
    )
    .unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_manifest_path).unwrap();
    let root_dev_block = root_dev_version.map_or_else(String::new, |version| {
        format!("    devDependencies:\n      foo:\n        specifier: 1.0.0\n        version: {version}\n")
    });
    let sibling_bar_block = sibling_dev_bar_version.map_or_else(String::new, |version| {
        format!("      bar:\n        specifier: 2.0.0\n        version: {version}\n")
    });
    fs::write(
        dir.path().join(Lockfile::FILE_NAME),
        format!(
            "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: {root_version}\n{root_dev_block}\n  pkg-a:\n    devDependencies:\n      foo:\n        specifier: 1.0.0\n        version: {sibling_version}\n{sibling_bar_block}",
        ),
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path())
        .expect("parse the two-importer lockfile")
        .expect("lockfile on disk");
    let settings = current_settings(config, pnpm_config::NodeLinker::Isolated, included, None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path()
            .to_string_lossy()
            .into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        sibling_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_validated_files(dir.path()), settings, projects);

    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: &[
            (dir.path().to_path_buf(), &root_manifest),
            (sibling_dir, &sibling_manifest),
        ],
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        catalogs: &BTreeMap::default(),
        layout: crate::RepeatInstallLayout {
            node_linker: pnpm_config::NodeLinker::Isolated,
            included,
            supported_architectures: None,
        },
        manifest_freshness: crate::ManifestFreshness::Mtime,
    })
}
