use super::{
    super::{Install, InstallError, ProjectMutation},
    InstallDirs, PARTIAL_INSTALL_LOCKFILE, seed_placeholder_virtual_store_slot,
};
use crate::PolicyExcludes;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use tempfile::tempdir;
use text_block_macros::text_block;

#[tokio::test]
async fn should_error_when_frozen_lockfile_is_requested_but_none_exists() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = true;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
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

    assert!(matches!(result, Err(InstallError::NoLockfile)));
    drop(dirs.dir);
}
#[tokio::test]
async fn should_error_when_frozen_lockfile_and_update_checksums_are_both_set() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = true;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: true,
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

    assert!(matches!(result, Err(InstallError::FrozenLockfileWithUpdateChecksums)));
    drop(dirs.dir);
}
/// `--frozen-lockfile` passed on the CLI must take precedence over
/// `config.lockfile=false`. Before this fix the dispatch matched on
/// `(config.lockfile, frozen_lockfile, lockfile)` in an order that
/// treated `config.lockfile=false` as "skip lockfile entirely",
/// silently dropping the CLI flag and resolving from the registry
/// instead — the very regression the integrated benchmark was
/// measuring. Pin the new priority: frozen flag + lockfile present
/// → `InstallFrozenLockfile`, regardless of `config.lockfile`.
///
/// We don't need the full install to succeed here — any error that
/// *isn't* `NoLockfile` proves the dispatch picked the frozen path.
/// Passing a malformed lockfile integrity surfaces as
/// `FrozenLockfile(...)`.
#[tokio::test]
async fn frozen_lockfile_flag_overrides_config_lockfile_false() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    // Explicitly disabled — this is the pacquet default today. The
    // CLI flag must still take over.
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    // Minimal v9 lockfile with no snapshots — the frozen path will
    // run through `CreateVirtualStore` with an empty snapshot set,
    // which is a successful no-op. That's enough to prove we took
    // the frozen branch.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies: {}"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse minimal v9 lockfile");

    Install {
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
    .await
    .expect("--frozen-lockfile + empty lockfile should succeed via InstallFrozenLockfile");

    drop(dirs.dir);
}
/// Symmetric negative: `--frozen-lockfile` with no lockfile
/// loadable must surface `NoLockfile`, even when `config.lockfile`
/// is `false` (which used to fall through to the no-lockfile path
/// and silently succeed).
#[tokio::test]
async fn frozen_lockfile_flag_with_no_lockfile_errors() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
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

    assert!(matches!(result, Err(InstallError::NoLockfile)));
    drop(dirs.dir);
}
/// Issue [#447]: a `--frozen-lockfile` install where the on-disk
/// `package.json` has drifted from the lockfile importer entry must
/// fail with `OutdatedLockfile` (`ERR_PNPM_OUTDATED_LOCKFILE`)
/// *before* any fetch or link work starts — a CI-correctness
/// guarantee that pacquet can't silently install the wrong shape of
/// `node_modules` when the manifest and lockfile diverge.
///
/// We use the partial-install fixture (bogus tarball URL) and *omit*
/// adding the placeholder dep to the manifest. If the check fails to
/// fire, the install reaches the fetch site and errors with a
/// network / integrity failure — distinguishable from the early
/// `OutdatedLockfile` we expect.
///
/// `trust_lockfile` is on so lockfile-resolution verification is
/// skipped: the unconditional tarball-URL binding check would otherwise
/// flag the fixture's tripwire tarball URL as a `TARBALL_URL_MISMATCH`
/// before the drift gate runs. Verification is orthogonal to the drift
/// check this test exercises.
///
/// [#447]: https://github.com/pnpm/pacquet/issues/447
#[tokio::test]
pub(super) async fn frozen_lockfile_errors_when_manifest_drifts_from_lockfile() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    // Deliberately do NOT add the `placeholder` dep — this is the
    // drift case the check has to catch.
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

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
        trust_lockfile: true,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
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

    let err = result.expect_err("drifted manifest must surface as OutdatedLockfile");
    assert!(
        matches!(err, InstallError::OutdatedLockfile { .. }),
        "expected OutdatedLockfile, got {err:?}",
    );

    drop(dirs.dir);
}
/// `pnpm.overrides` drift between the lockfile-recorded map and the
/// current config surfaces as `OutdatedLockfile` with a
/// `StalenessReason::OverridesChanged` payload under `--frozen-lockfile`.
#[tokio::test]
async fn frozen_lockfile_errors_when_overrides_drift_from_lockfile() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    // Config declares an override the lockfile doesn't carry → drift.
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("placeholder".to_string(), "9.9.9".to_string());
    config.overrides = Some(overrides);
    let config = config.leak();

    // Lockfile fixture has *no* `overrides:` key.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .: {}"
    })
    .expect("parse minimal lockfile");

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
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
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

    let err = result.expect_err("overrides drift must surface as a config mismatch");
    match err {
        InstallError::LockfileConfigMismatch { setting: "overrides" } => {}
        other => panic!("expected LockfileConfigMismatch for `overrides`, got {other:?}"),
    }

    drop(dirs.dir);
}
/// When `pnpm.overrides` is set, the freshness check applies overrides
/// to a clone of the manifest before comparing against the lockfile.
/// Without this step, the lockfile's post-override specifier and the
/// manifest's pre-override specifier would always disagree, failing
/// every frozen install with `SpecifiersDiffer`. This test pins that
/// behavior: a manifest declaring `foo: ^1` plus an override
/// `foo: 2.0.0` lines up with a lockfile that records `foo: 2.0.0` —
/// after override application — and the install proceeds.
#[tokio::test]
async fn frozen_lockfile_applies_overrides_to_manifest_before_freshness_check() {
    let dirs = InstallDirs::new();
    seed_placeholder_virtual_store_slot(&dirs.virtual_store_dir);

    let manifest_path = dirs.path().join("package.json");
    // Manifest lists `placeholder: ^9` (pre-override). Without
    // override application this would trip the freshness check
    // because the lockfile records `placeholder: 1.0.0`.
    std::fs::write(
        &manifest_path,
        r#"{"name":"my-app","version":"1.0.0","dependencies":{"placeholder":"^9"}}"#,
    )
    .unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("placeholder".to_string(), "1.0.0".to_string());
    config.overrides = Some(overrides);
    let config = config.leak();

    // Lockfile carries the SAME override map so the drift check
    // passes; the importer's specifier reflects the post-override
    // value `1.0.0`.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  placeholder: 1.0.0"
        "importers:"
        "  .:"
        "    dependencies:"
        "      placeholder:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  placeholder@1.0.0:"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA, tarball: 'http://invalid.local/placeholder.tgz'}"
        "snapshots:"
        "  placeholder@1.0.0: {}"
    })
    .expect("parse fixture lockfile with overrides");

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
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
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

    // The install should not fail with `OutdatedLockfile` — the
    // overrider rewrites `placeholder: ^9 → 1.0.0` on the cloned
    // manifest, lining up with the lockfile. (The install may still
    // fail later for unrelated reasons in this minimal fixture, but
    // the freshness gate must pass.)
    if let Err(InstallError::OutdatedLockfile { reason }) = &result {
        panic!("unexpected OutdatedLockfile after override application: {reason:?}");
    }

    drop(dirs.dir);
}
/// `pnpm.overrides` values can reference a workspace catalog via the
/// `catalog:` protocol; pnpm resolves them against `catalogs:` in
/// `pnpm-workspace.yaml` and writes the *resolved* specifier to
/// `pnpm-lock.yaml#overrides`. The freshness check must therefore
/// resolve `catalog:` on the config side too before comparing — a
/// raw string compare would treat `catalog:` ≠ `<concrete>` on every
/// install. The config side parses the overrides against the catalogs
/// and builds the resolved overrides map before comparing. Regression
/// test for the case the user hit on a workspace whose `pnpm.overrides`
/// declared catalog-backed entries.
#[tokio::test]
async fn frozen_lockfile_resolves_catalog_protocol_in_overrides_before_freshness_check() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    std::fs::create_dir_all(&project_root).expect("create project root");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    seed_placeholder_virtual_store_slot(&virtual_store_dir);

    // The catalog lives in `pnpm-workspace.yaml` next to the manifest;
    // `Install::run` walks up from the manifest dir to find it.
    std::fs::write(
        project_root.join("pnpm-workspace.yaml"),
        text_block! {
            "catalogs:"
            "  default:"
            "    placeholder: 1.0.0"
        },
    )
    .unwrap();

    let manifest_path = project_root.join("package.json");
    std::fs::write(
        &manifest_path,
        r#"{"name":"my-app","version":"1.0.0","dependencies":{"placeholder":"^9"}}"#,
    )
    .unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir;
    // Override value is `catalog:`, which must resolve to the
    // catalog's `placeholder: 1.0.0` entry before the freshness
    // comparison. The lockfile records the *resolved* `1.0.0`, so a
    // raw compare would fail on every install.
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("placeholder".to_string(), "catalog:".to_string());
    config.overrides = Some(overrides);
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  placeholder: 1.0.0"
        "importers:"
        "  .:"
        "    dependencies:"
        "      placeholder:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  placeholder@1.0.0:"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA, tarball: 'http://invalid.local/placeholder.tgz'}"
        "snapshots:"
        "  placeholder@1.0.0: {}"
    })
    .expect("parse fixture lockfile with overrides");

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
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
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

    // The freshness check must accept the install; before the fix
    // pacquet would surface `OutdatedLockfile::OverridesChanged`
    // because `catalog:` ≠ `1.0.0` as raw strings. (The install may
    // still fail later for unrelated reasons in this minimal fixture,
    // but the overrides gate must pass.)
    if let Err(InstallError::OutdatedLockfile { reason }) = &result {
        panic!("unexpected OutdatedLockfile after catalog resolution: {reason:?}");
    }

    drop(dir);
}
/// Negative-case: lockfile loads successfully but has no
/// `importers["."]` entry for the project being installed. Distinct
/// from `NoLockfile` (file missing entirely) — here the file is
/// well-formed but doesn't describe this project. Should surface as
/// `NoImporter`, also before any fetch attempt.
#[tokio::test]
async fn frozen_lockfile_errors_when_lockfile_has_no_root_importer() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    // Empty-importers lockfile — valid v9 shape, but no entry for
    // the root project.
    let lockfile: Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse minimal lockfile");

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
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
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

    let err = result.expect_err("missing root importer must surface as NoImporter");
    assert!(
        matches!(err, InstallError::NoImporter { ref importer_id } if importer_id == "."),
        "expected NoImporter for `.`, got {err:?}",
    );

    drop(dirs.dir);
}
/// GVS-on frozen-lockfile install. With
/// `enable_global_virtual_store: true` (an explicit opt-in;
/// pacquet's default is `false`, matching pnpm v11's effective
/// default for non-`--global` installs — see
/// [`pnpm_config::default_enable_global_virtual_store`]),
/// `Install::run` registers the project at
/// `<store_dir>/projects/<short-hash>`
/// and routes every per-snapshot slot through
/// [`crate::VirtualStoreLayout`]. The empty-snapshot lockfile here is
/// enough to prove the wiring runs end-to-end without panicking and
/// that the registry entry actually lands on disk; the GVS-shaped
/// per-package path layout itself is unit-tested inside the
/// [`crate::VirtualStoreLayout`] module, and the e2e GVS cases (with
/// non-empty snapshots) are tracked as a follow-up.
#[tokio::test]
async fn frozen_lockfile_under_gvs_registers_project_and_runs_clean() {
    let dirs = InstallDirs::new();

    // Place the manifest *inside* `dirs.project_root` — `Install::run`
    // derives the registry target from `manifest.path().parent()`,
    // so a manifest at `<tmp>/package.json` would register `<tmp>`
    // and the symlink-resolves-to-dirs.project_root assertion below
    // would silently pass for the wrong reason.
    std::fs::create_dir_all(&dirs.project_root).expect("create project root");
    let manifest_path = dirs.project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    // Pin GVS on explicitly — pacquet's default is `false`, so this
    // test would test the wrong path otherwise.
    config.enable_global_virtual_store = true;
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    // Pin the GVS root to a known location under the test temp dirs.dir
    // so any future assertions can target it without walking the
    // SmartDefault'd cwd-based fallback.
    config.global_virtual_store_dir = dirs.store_dir.join("links");
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies: {}"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse minimal v9 lockfile");

    Install {
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
        resolved_packages: &Default::default(),
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        policy_excludes: PolicyExcludes::Persist,
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
    .expect("frozen-lockfile install under GVS should succeed");

    // `register_project` wrote `<dirs.store_dir>/v11/projects/<short-hash>`
    // pointing back at the project dirs.dir. Canonicalize the *entry
    // path* (not `read_link`'s output) so the kernel follows the
    // symlink — pacquet writes the target as
    // a path relative to the link's parent, so canonicalizing the
    // raw `read_link` string from the CWD would never resolve.
    let projects_dir = dirs.store_dir.join("v11/projects");
    assert!(projects_dir.is_dir(), "GVS-on install must create <dirs.store_dir>/v11/projects/");
    let entries: Vec<_> =
        std::fs::read_dir(&projects_dir).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(entries.len(), 1, "exactly one project entry per `Install::run` invocation");
    assert_eq!(
        dunce::canonicalize(entries[0].path()).expect("canonicalize registry entry"),
        dunce::canonicalize(&dirs.project_root).expect("canonicalize project root"),
        "registry symlink must resolve back to the install's project root",
    );

    drop(dirs.dir);
}
