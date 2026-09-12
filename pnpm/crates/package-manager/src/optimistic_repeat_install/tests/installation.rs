use super::{
    super::{
        Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install,
        deps_status::install_args_from_state,
        settings::current_settings,
        timestamps::{FileMtime, modified_at_or_after},
    },
    assert_deps_status_converges_after_collision, check, check_with_lockfile,
    content_check_decision, isolated_included, setup_content_check_project, setup_fresh_install,
    setup_fresh_install_with_config, validate_existing_files, write_empty_lockfile,
    write_registry_lockfile, write_state,
};
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_lockfile::MaybeLazyLockfile;
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::backdate_existing_files;
use pnpm_workspace_state::{ProjectEntry, WorkspaceState, WorkspaceStateSettings};
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

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
#[test]
fn run_gate_converges_after_a_same_millisecond_mtime_collision() {
    assert_deps_status_converges_after_collision(500_000);
}
#[test]
fn run_gate_converges_after_a_whole_second_mtime_collision() {
    assert_deps_status_converges_after_collision(0);
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
