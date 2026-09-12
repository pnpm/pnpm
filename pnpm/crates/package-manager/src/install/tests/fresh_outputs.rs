use super::{
    super::{Install, InstallError, ProjectMutation},
    InstallDirs, is_modules_yaml_consistent, is_modules_yaml_layout_consistent,
};
use crate::PolicyExcludes;
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{
    Host, LayoutVersion, Modules, NodeLinker, read_modules_manifest, write_modules_manifest,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use pnpm_testing_utils::{fs::is_symlink_or_junction, registry::TestRegistry};
use tempfile::tempdir;
use text_block_macros::text_block;

/// End-to-end wiring smoke for the lockfile-verification gate
/// (Phase 7). An invalid `minimumReleaseAgeExclude` pattern (the
/// glob form is rejected when paired with a version part, surfacing
/// `ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION`) trips
/// `build_resolution_verifiers` before the frozen-lockfile dispatch
/// runs. The resulting `InstallError::BuildVerifiers` proves:
///
/// 1. `build_resolution_verifiers` actually fires during install.
/// 2. The error short-circuits the install — no virtual-store
///    materialization, no registry round-trip.
///
/// The gate's positive / negative `verify_lockfile_resolutions`
/// branches are exercised by the unit tests in
/// `pnpm-lockfile-verification`; this test pins only the install
/// wiring so it stays fast and doesn't depend on the mocked
/// packument shape.
#[tokio::test]
async fn install_rejects_invalid_minimum_release_age_exclude_pattern() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    // Activate the verifier with an invalid exclude entry — the
    // version-part-with-wildcard combination is rejected by
    // `create_package_version_policy`.
    config.minimum_release_age = Some(60);
    config.minimum_release_age_exclude = Some(vec!["is-*@1.0.0".to_string()]);
    let config = config.leak();

    // Empty lockfile is enough — the gate runs as soon as
    // `lockfile.is_some()` regardless of the snapshot count.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies: {}"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse minimal v9 lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<SilentReporter>()
    .await;

    let err = result.expect_err("invalid exclude pattern must surface");
    assert!(matches!(err, InstallError::BuildVerifiers(_)), "expected BuildVerifiers, got {err:?}");
    assert!(
        !dirs.project_root.join("node_modules/.pacquet").exists(),
        "BuildVerifiers must abort before virtual-store materialization",
    );

    drop(dirs.dir);
}
// ----------------------------------------------------------------------------
// Fresh-install lockfile generation
//
// These tests exercise a *fresh* install — the path that converts the
// resolver's `DependenciesGraph` into a v9 `pnpm-lock.yaml`. Tests that
// require an existing lockfile (incremental update, repeat install,
// `--frozen-lockfile`-with-stale-lockfile) stay deferred — see issue
// pnpm/pnpm#11813 for the broader scope.

/// Scenario: lockfile has correct format.
/// The smallest fresh-install slice: one direct prod dep produces
/// a v9 lockfile with the right `lockfileVersion`, an importer entry
/// under `.`, and matching `packages:` / `snapshots:` rows.
#[tokio::test]
async fn fresh_install_writes_pnpm_lock_yaml_with_expected_shape() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dirs.path().join(Lockfile::FILE_NAME);
    assert!(lockfile_path.is_file(), "pnpm-lock.yaml must be written next to the manifest");

    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse fresh lockfile");

    assert_eq!(lockfile.lockfile_version.major, 9);
    let importer = lockfile.root_project().expect("root importer recorded");
    let deps = importer.dependencies.as_ref().expect("dependencies map");
    let hello_key: pnpm_lockfile::PkgName =
        pnpm_lockfile::PkgName::parse("@pnpm.e2e/hello-world-js-bin").unwrap();
    let entry = deps.get(&hello_key).expect("hello-world-js-bin recorded");
    assert_eq!(entry.specifier, "1.0.0");

    let packages = lockfile.packages.as_ref().expect("packages map populated");
    let pkg_key: pnpm_lockfile::PackageKey = "@pnpm.e2e/hello-world-js-bin@1.0.0".parse().unwrap();
    let metadata = packages.get(&pkg_key).expect("packages entry");
    assert!(metadata.resolution.integrity().is_some(), "registry resolution carries integrity");

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map populated");
    assert!(
        snapshots.contains_key(&pkg_key),
        "snapshot keyed by depPath (pure pkg id when no peers)",
    );

    drop((dirs.dir, mock_instance));
}
/// Manifest-declared dependency groups land in the matching importer
/// section in the lockfile (packages are placed in `devDependencies`
/// even if they are present as non-dev as well). Pacquet routes deps
/// through `manifest_alias_to_group`, so a dep declared in
/// `devDependencies` lands in the lockfile's `devDependencies` section.
#[tokio::test]
async fn fresh_install_splits_dev_and_prod_dependency_sections() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.add_dependency("@pnpm/xyz", "1.0.0", DependencyGroup::Dev).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dirs.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse fresh lockfile");

    let importer = lockfile.root_project().expect("root importer");
    let prod = importer.dependencies.as_ref().expect("prod section");
    let hello_key = pnpm_lockfile::PkgName::parse("@pnpm.e2e/hello-world-js-bin").unwrap();
    let xyz_key = pnpm_lockfile::PkgName::parse("@pnpm/xyz").unwrap();
    assert!(prod.contains_key(&hello_key));
    assert!(!prod.contains_key(&xyz_key), "dev dep stays out of the prod section");
    let dev = importer.dev_dependencies.as_ref().expect("dev section");
    assert!(dev.contains_key(&xyz_key));

    drop((dirs.dir, mock_instance));
}
/// A top-level `optionalDependencies` entry surfaces as
/// `snapshots[<key>].optional: true` in the freshly-written lockfile.
/// The `optional` flag is what `BuildModules` consults to decide
/// whether a build failure should be reported via
/// `pnpm:skipped-optional-dependency`. A non-optional sibling lands
/// `optional: false` so the test pins both sides.
#[tokio::test]
async fn fresh_install_marks_optional_snapshots_in_pnpm_lock_yaml() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.add_dependency("@pnpm/xyz", "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let lockfile_path = dirs.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse lockfile");
    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");

    // Look snapshots up by package name + version rather than by an
    // exact key parse: `@pnpm/xyz` declares peer deps, so the resolver
    // emits a peer-suffixed depPath that won't parse cleanly out of a
    // literal here. Membership-by-name keeps the test robust to the
    // fixture's exact peer-suffix shape.
    let find_optional = |scope: &str, bare: &str| -> Option<bool> {
        snapshots
            .iter()
            .find(|(key, _)| key.name.scope.as_deref() == Some(scope) && key.name.bare == bare)
            .map(|(_, entry)| entry.optional)
    };

    assert_eq!(
        find_optional("pnpm.e2e", "hello-world-js-bin"),
        Some(false),
        "non-optional direct dep must land with optional: false",
    );
    assert_eq!(
        find_optional("pnpm", "xyz"),
        Some(true),
        "optionalDependencies entry must propagate to snapshots[<key>].optional",
    );
    // Note: transitive deps that arrive via auto-install-peers
    // hoisting land at the importer level as non-optional — they're
    // installed top-level to satisfy a missing peer regardless of
    // whether the consumer was optional (the hoist
    // semantics). Pure transitive optional propagation
    // (consumer → regular `dependencies` child) is exercised by the
    // adapter's unit tests since the mock-registry fixtures here
    // don't expose that shape.

    drop((dirs.dir, mock_instance));
}
#[tokio::test]
async fn fresh_install_skips_platform_incompatible_optional_dependency() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest
        .add_dependency("@pnpm.e2e/not-compatible-with-any-os", "1.0.0", DependencyGroup::Optional)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = mock_instance.url();
    let config = config.leak();

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
        resolved_packages: &Default::default(),
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        auth_override: None,
        resolution_observer: None,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: None,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    assert!(
        is_symlink_or_junction(
            &dirs.project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin")
        )
        .unwrap(),
        "compatible prod dependency should be linked",
    );
    assert!(
        !dirs.project_root.join("node_modules/@pnpm.e2e/not-compatible-with-any-os").exists(),
        "platform-incompatible optional dependency must not be linked",
    );
    assert!(
        !dirs.virtual_store_dir.join("@pnpm.e2e+not-compatible-with-any-os@1.0.0").exists(),
        "platform-incompatible optional dependency must not be extracted",
    );

    let lockfile_path = dirs.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(&lockfile_path).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&content).expect("parse lockfile");
    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let skipped_key = snapshots
        .iter()
        .find(|(key, _)| {
            key.name.scope.as_deref() == Some("pnpm.e2e")
                && key.name.bare == "not-compatible-with-any-os"
        })
        .map(|(key, snapshot)| {
            assert!(snapshot.optional, "lockfile snapshot should stay marked optional");
            key.clone()
        })
        .expect("optional dependency should stay in the lockfile");

    let written = dirs
        .modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");
    // The recorded skip set is the reachability closure: the skipped
    // optional's own dependency is not materialized either.
    assert_eq!(
        written.skipped,
        ["@pnpm.e2e/dep-of-optional-pkg@1.0.0".to_string(), skipped_key.to_string()],
    );

    let current_lockfile_path = dirs.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
    let current_content =
        std::fs::read_to_string(&current_lockfile_path).expect("read current lockfile");
    let current_lockfile: Lockfile =
        serde_saphyr::from_str(&current_content).expect("parse current lockfile");
    // `.modules.yaml.skipped`, asserted above, is what carries the skip.
    assert_eq!(current_lockfile, lockfile);

    drop((dirs.dir, mock_instance));
}
/// [`is_modules_yaml_consistent`] returns `false` when
/// `.modules.yaml` is missing, so a first install (no prior state)
/// can't be mistaken for an up-to-date install.
#[test]
fn is_modules_yaml_consistent_returns_false_when_modules_yaml_absent() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");
    let mut config = Config::new();
    config.modules_dir = modules_dir.clone();
    let config = config.leak();

    assert!(!is_modules_yaml_consistent(
        &modules_dir,
        config,
        pnpm_config::NodeLinker::default(),
        pnpm_modules_yaml::IncludedDependencies::default(),
    ));
}
/// [`is_modules_yaml_consistent`] returns `true` when every
/// layout-determining setting matches what
/// [`super::super::build_modules_manifest`] would write for the current
/// config / linker / dependency-group selection. The roundtrip needs
/// to be exact because a single drifted setting forces the next
/// install to rebuild the modules directory.
#[test]
fn is_modules_yaml_consistent_returns_true_when_settings_match() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let included = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: true,
    };

    let seed = Modules {
        layout_version: Some(LayoutVersion),
        node_linker: Some(NodeLinker::Isolated),
        included,
        hoist_pattern: config.hoist_pattern.clone(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        ..Default::default()
    };
    write_modules_manifest::<Host>(&modules_dir, seed).expect("seed .modules.yaml");

    assert!(is_modules_yaml_consistent(
        &modules_dir,
        config,
        pnpm_config::NodeLinker::default(),
        included,
    ));
}
/// Dependency-group drift between `.modules.yaml.included` and the
/// current install request disqualifies the short-circuit. A
/// previously installed `--no-optional` setup can't be re-used to
/// satisfy an install that needs optional dependencies.
#[test]
fn is_modules_yaml_consistent_returns_false_when_included_drifts() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let prod_only = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };
    let with_optional = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: true,
    };

    let seed = Modules {
        layout_version: Some(LayoutVersion),
        node_linker: Some(NodeLinker::Isolated),
        included: prod_only,
        hoist_pattern: config.hoist_pattern.clone(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        ..Default::default()
    };
    write_modules_manifest::<Host>(&modules_dir, seed).expect("seed .modules.yaml");

    assert!(!is_modules_yaml_consistent(
        &modules_dir,
        config,
        pnpm_config::NodeLinker::Isolated,
        with_optional,
    ));
}
/// A `--prod`<->full switch changes `.modules.yaml.included`, which the
/// up-to-date fast path must still notice (so it relinks). But it must NOT
/// make the *layout* look inconsistent: pnpm never purges the root
/// project's `node_modules` for an included mismatch (validateModules's
/// `lockfileDir !== rootDir` guard), and the purge wipes the user's
/// non-pnpm entries (a vendored dir, custom files). So an included-only
/// drift keeps the layout consistent — the purge does not run and the
/// directory is left untouched; relinking adds/removes the right deps.
#[test]
fn included_drift_alone_does_not_make_the_layout_inconsistent() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let prod_only = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };
    let with_optional = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: true,
    };

    let seed = Modules {
        layout_version: Some(LayoutVersion),
        node_linker: Some(NodeLinker::Isolated),
        included: prod_only,
        hoist_pattern: config.hoist_pattern.clone(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        ..Default::default()
    };
    write_modules_manifest::<Host>(&modules_dir, seed).expect("seed .modules.yaml");

    // The fast path still sees the included change and forces a reinstall...
    assert!(!is_modules_yaml_consistent(
        &modules_dir,
        config,
        pnpm_config::NodeLinker::Isolated,
        with_optional,
    ));
    // ...but the layout is still consistent, so the destructive purge does
    // not run and the user's node_modules contents survive.
    assert!(is_modules_yaml_layout_consistent(
        &modules_dir,
        config,
        pnpm_config::NodeLinker::Isolated,
    ));
}
/// A real layout setting (here `nodeLinker`) drifting still makes the
/// layout inconsistent, so the purge that recreates `node_modules` runs —
/// the included guard above must not suppress genuine layout rebuilds.
#[test]
fn layout_drift_still_makes_the_layout_inconsistent() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    let config = config.leak();

    let seed = Modules {
        layout_version: Some(LayoutVersion),
        node_linker: Some(NodeLinker::Hoisted),
        hoist_pattern: config.hoist_pattern.clone(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        ..Default::default()
    };
    write_modules_manifest::<Host>(&modules_dir, seed).expect("seed .modules.yaml");

    assert!(!is_modules_yaml_layout_consistent(
        &modules_dir,
        config,
        pnpm_config::NodeLinker::Isolated,
    ));
}
