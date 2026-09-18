use super::{
    super::{
        Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install,
        conflict_markers::MAX_LOCKFILE_CONFLICT_SCAN_BYTES,
        deps_status::{RunDepsStatus, check_deps_status_before_run},
        settings::current_settings,
        timestamps::{FileMtime, lockfile_modified_since, modified_at_or_after},
    },
    FOO_LOCKFILE, FOO_LOCKFILE_WITHOUT_PACKAGES, FOO_MANIFEST, backdate_validated_files, check,
    check_with_lockfile, check_workspace, content_check_decision, isolated_included,
    setup_content_check_project, setup_fresh_install, setup_fresh_install_with_config,
    validate_existing_files, write_bare_tarball_lockfile, write_state,
};
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::set_mtime_ms;
use pnpm_workspace_state::{ProjectEntry, load_workspace_state, update_workspace_state};
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

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
    write_state(workspace_root, backdate_validated_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
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
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
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
            project_manifests: &[(dir.path().to_path_buf(), &manifest)],
            is_workspace_install: false,
            lockfile: MaybeLazyLockfile::Loaded(None),
            catalogs: &BTreeMap::default(),
            layout: crate::RepeatInstallLayout {
                node_linker: pnpm_config::NodeLinker::Isolated,
                included: isolated_included(),
                supported_architectures: None,
            },
            manifest_freshness: crate::ManifestFreshness::Mtime,
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
    write_state(workspace_root, backdate_validated_files(workspace_root), settings, projects);

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
/// A wanted lockfile rewritten after the last install may still describe
/// the tree on disk: a snapshot no importer reaches is never materialized,
/// and a top-level key pnpm does not define (an embedder's extension block,
/// as Bit records beside pnpm's keys) is never written to the current
/// lockfile. Neither difference makes the installed dependencies outdated.
#[test]
fn returns_up_to_date_when_the_newer_wanted_lockfile_differs_only_in_unmaterialized_content() {
    let (dir, config) = setup_content_check_project();

    fs::write(
        dir.path().join(Lockfile::FILE_NAME),
        FOO_LOCKFILE_WITH_UNREACHABLE_BAR_AND_EXTENSION,
    )
    .unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(decision, Decision::UpToDate);
}

/// [`FOO_LOCKFILE`] plus a `bar@1.0.0` no importer depends on and a `bit:`
/// block pnpm does not define; what it materializes is [`FOO_LOCKFILE`].
const FOO_LOCKFILE_WITH_UNREACHABLE_BAR_AND_EXTENSION: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  bar@1.0.0:
    resolution: {integrity: sha512-bbb}

  foo@1.0.0:
    resolution: {integrity: sha512-aaa}

snapshots:

  bar@1.0.0: {}

  foo@1.0.0: {}

bit:
  depsRequiringBuild: []
";
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
    assert!(
        dir.path()
            .join(Lockfile::FILE_NAME)
            .exists(),
        "pnpm-lock.yaml must be regenerated",
    );
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
        !dir.path()
            .join(Lockfile::FILE_NAME)
            .exists(),
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
    assert!(
        dir.path()
            .join(Lockfile::FILE_NAME)
            .exists(),
        "pnpm-lock.yaml must be regenerated",
    );
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
    assert!(
        !dir.path()
            .join(Lockfile::FILE_NAME)
            .exists(),
        "lockfile: false must skip the write",
    );
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
/// The mtime the install's own lockfile write lands on in the tests
/// below. Deliberately not a whole second: a whole-second lockfile mtime
/// counts its entire second as possibly-modified, which would route every
/// project into the content check and mask what these tests assert.
const COMMITTING_LOCKFILE_MS: i64 = 1_700_000_000_123;

/// An install records `lastValidatedTimestamp` once it has committed
/// everything, which is later than the lockfile it wrote and later still
/// than the manifests it read. A dependency edit that lands in that gap is
/// absent from the lockfile, so the single-project fast path has to see it
/// ([#14890](https://github.com/pnpm/pnpm/issues/14890)).
fn setup_edit_during_install() -> (tempfile::TempDir, &'static Config, PackageManifest) {
    let (dir, config) = setup_content_check_project();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"root","version":"1.0.0","dependencies":{"foo":"^1.0.0","bar":"^2.0.0"}}"#,
    )
    .unwrap();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
    set_mtime_ms(&dir.path().join(Lockfile::FILE_NAME), COMMITTING_LOCKFILE_MS);
    set_mtime_ms(
        &config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME),
        COMMITTING_LOCKFILE_MS,
    );
    set_mtime_ms(&dir.path().join("package.json"), COMMITTING_LOCKFILE_MS + 250);

    let mut state = load_workspace_state(dir.path()).unwrap().unwrap();
    state.last_validated_timestamp = COMMITTING_LOCKFILE_MS + 500;
    update_workspace_state(dir.path(), &state).unwrap();

    (dir, config, manifest)
}
#[test]
fn install_detects_a_manifest_edit_that_landed_while_the_install_was_committing() {
    let (dir, config, manifest) = setup_edit_during_install();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);

    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("satisfied")),
        "expected Skipped(no longer satisfied), got {decision:?}",
    );
}
/// `verifyDepsBeforeRun` must reach the same verdict the install fast path
/// does, or `pnpm run` executes scripts against a `node_modules` that no
/// longer matches the manifest.
#[test]
fn run_gate_detects_a_manifest_edit_that_landed_while_the_install_was_committing() {
    let (dir, config, manifest) = setup_edit_during_install();
    let state = load_workspace_state(dir.path()).unwrap().unwrap();

    let status = check_deps_status_before_run(
        &OptimisticRepeatInstallCheck {
            workspace_root: dir.path(),
            config,
            project_manifests: &[(dir.path().to_path_buf(), &manifest)],
            is_workspace_install: false,
            lockfile: MaybeLazyLockfile::Loaded(None),
            catalogs: &BTreeMap::default(),
            layout: crate::RepeatInstallLayout {
                node_linker: pnpm_config::NodeLinker::Isolated,
                included: isolated_included(),
                supported_architectures: None,
            },
            manifest_freshness: crate::ManifestFreshness::Mtime,
        },
        &state,
    );

    assert!(
        matches!(&status, RunDepsStatus::Outdated { issue, .. } if issue.contains("satisfied")),
        "expected Outdated(no longer satisfied), got {status:?}",
    );
}
/// Measuring against the lockfile only widens the window a manifest edit
/// is caught in. An untouched manifest older than the lockfile still takes
/// the pure-mtime fast path.
#[test]
fn keeps_the_fast_path_for_a_manifest_older_than_the_lockfile() {
    let (dir, config) = setup_content_check_project();
    let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
    set_mtime_ms(&dir.path().join("package.json"), COMMITTING_LOCKFILE_MS);
    set_mtime_ms(&dir.path().join(Lockfile::FILE_NAME), COMMITTING_LOCKFILE_MS + 250);
    set_mtime_ms(
        &config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME),
        COMMITTING_LOCKFILE_MS + 250,
    );

    let mut state = load_workspace_state(dir.path()).unwrap().unwrap();
    state.last_validated_timestamp = COMMITTING_LOCKFILE_MS + 500;
    update_workspace_state(dir.path(), &state).unwrap();

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );

    assert_eq!(decision, Decision::UpToDate);
}
#[test]
fn detects_an_edit_during_install_against_the_current_lockfile_stand_in() {
    let (dir, config, manifest) = setup_edit_during_install();
    fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).unwrap();

    let decision =
        content_check_decision(&dir, config, false, &[(dir.path().to_path_buf(), &manifest)]);

    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("satisfied")),
        "expected Skipped(no longer satisfied), got {decision:?}",
    );
}
