use super::{
    super::{Install, InstallError, ProjectMutation},
    InstallDirs, PARTIAL_INSTALL_LOCKFILE, seed_placeholder_virtual_store_slot,
};
use crate::PolicyExcludes;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{Host, LayoutVersion, Modules, NodeLinker, write_modules_manifest};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, Reporter, SilentReporter, Stage};
use pnpm_testing_utils::registry::TestRegistry;
use pnpm_workspace_state::load_workspace_state;
use std::{fs, sync::Mutex, time::Duration};
use text_block_macros::text_block;

/// Frozen-lockfile install with a `VariationsResolution` whose
/// variants only target a platform pacquet CI never runs on
/// (`aix/ppc64`) must surface
/// [`crate::InstallPackageBySnapshotError::NoMatchingPlatformVariant`]
/// from the cold-batch dispatcher. Variant selection happens
/// before any network fetch, so the bogus URL on the variant is
/// never read — the test stays hermetic.
///
/// Closes the variant-mismatch checkbox of [#437] slice F.
///
/// [#437]: https://github.com/pnpm/pacquet/issues/437
#[tokio::test]
async fn frozen_lockfile_install_errors_when_no_variant_matches_host() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("node", "runtime:22.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    // Lockfile with a runtime entry whose variants only target a
    // platform we're not running on. `runtime:22.0.0` is preserved
    // through `PkgVerPeer`'s `Prefix::Runtime` (<https://github.com/pnpm/pacquet/issues/511> / <https://github.com/pnpm/pacquet/pull/512>); the
    // depPath round-trips correctly through pacquet's parser.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      node:"
        "        specifier: 'runtime:22.0.0'"
        "        version: 'runtime:22.0.0'"
        "packages:"
        "  'node@runtime:22.0.0':"
        "    hasBin: true"
        "    resolution:"
        "      type: variations"
        "      variants:"
        "        - resolution:"
        "            type: binary"
        "            url: 'https://example.test/node-aix-ppc64.tar.gz'"
        "            integrity: 'sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa=='"
        "            bin: 'bin/node'"
        "            archive: tarball"
        "          targets:"
        "            - os: aix"
        "              cpu: ppc64"
        "snapshots:"
        "  'node@runtime:22.0.0': {}"
    })
    .expect("parse variant-mismatch fixture lockfile");

    let err = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        resolved_packages: &Default::default(),
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
    .expect_err("variant-mismatch lockfile must surface a typed error");

    let rendered = format!("{err:?}");
    eprintln!("ERROR DEBUG:\n{rendered}");
    assert!(
        rendered.contains("NoMatchingPlatformVariant"),
        "expected NoMatchingPlatformVariant in the error chain, got: {rendered}",
    );
    let displayed = err.to_string();
    assert!(!displayed.is_empty(), "Display impl should produce a non-empty user-facing message");

    drop(dirs.dir);
}
/// Same lockfile + manifest shape as
/// [`frozen_lockfile_install_errors_when_no_variant_matches_host`],
/// but with `skip_runtimes: true`. The `--no-runtime` filter
/// iterates importer-direct deps, builds `node@runtime:22.0.0`
/// from `(alias, version)`, sees the `@runtime:` substring, and
/// adds the snapshot to the skip set — so variant selection
/// never runs and the unmatchable-platform variant doesn't fail
/// the install.
///
/// Closes the `--no-runtime` checkbox of [#437] slice F.
///
/// [#437]: https://github.com/pnpm/pacquet/issues/437
#[tokio::test]
async fn frozen_lockfile_install_skips_runtime_when_skip_runtimes_set() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("node", "runtime:22.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      node:"
        "        specifier: 'runtime:22.0.0'"
        "        version: 'runtime:22.0.0'"
        "packages:"
        "  'node@runtime:22.0.0':"
        "    hasBin: true"
        "    resolution:"
        "      type: variations"
        "      variants:"
        "        - resolution:"
        "            type: binary"
        "            url: 'https://example.test/node-aix-ppc64.tar.gz'"
        "            integrity: 'sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa=='"
        "            bin: 'bin/node'"
        "            archive: tarball"
        "          targets:"
        "            - os: aix"
        "              cpu: ppc64"
        "snapshots:"
        "  'node@runtime:22.0.0': {}"
    })
    .expect("parse --no-runtime fixture lockfile");

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
         lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        frozen_lockfile: true,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: true,
        trust_lockfile: false,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        resolved_packages: &Default::default(),
        node_linker: pnpm_config::NodeLinker::default(),
        lockfile_only: false,
        dry_run: false,
        update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
        preferred_versions_override: None,
        policy_excludes: PolicyExcludes::Persist,
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
    .expect("--no-runtime should skip the unmatchable runtime entry and let the rest of the install succeed");

    // The runtime slot must NOT exist under the virtual store —
    // the snapshot was filtered out by the skip set.
    //
    // Use `symlink_metadata` rather than `Path::exists()` so a
    // *dangling* symlink fails the assertion too: `exists()`
    // follows symlinks and reports `false` for a broken one, but
    // a broken symlink at `<dirs.modules_dir>/node` would still mean
    // the install created an entry the skip set was supposed to
    // suppress.
    let runtime_slot = dirs.virtual_store_dir.join("node@runtime:22.0.0");
    assert!(
        std::fs::symlink_metadata(&runtime_slot).is_err(),
        "runtime slot should not be materialized under --no-runtime, got {runtime_slot:?}",
    );
    // Neither should the direct-dep symlink under the project's
    // `node_modules/`. Same `symlink_metadata` rationale.
    let direct_dep = dirs.modules_dir.join("node");
    assert!(
        std::fs::symlink_metadata(&direct_dep).is_err(),
        "direct-dep symlink for node should not be created under --no-runtime, got {direct_dep:?}",
    );

    drop(dirs.dir);
}
/// Positive-path proof that `verify_lockfile_resolutions` runs from
/// inside `Install::run`. With `minimumReleaseAge` set absurdly high
/// (100 years), every version the mocked registry knows about is
/// inside the cutoff, so the gate rejects every lockfile entry
/// before any tarball is fetched.
///
/// Asserts:
///
/// 1. `Install::run` returns `Err(InstallError::LockfileVerification(...))`
///    with the inner `VerifyError::MinimumReleaseAgeViolation` —
///    i.e. the verifier code path actually ran and returned a
///    violation, the wiring is correct, and the dispatch maps the
///    inner code to the per-policy variant rather than collapsing
///    to the generic envelope.
/// 2. No virtual-store materialization. The gate fails before
///    `InstallFrozenLockfile` runs, so neither the slot nor the
///    project's `node_modules` symlink exist.
#[tokio::test]
async fn frozen_lockfile_gate_rejects_under_huge_minimum_release_age() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = mock_instance.url();
    // 100 years in minutes. Anything the registry has shipped to
    // date is inside the cutoff, so the publish-time check rejects
    // every lockfile entry regardless of what the mocked packument's
    // `time` map actually says.
    config.minimum_release_age = Some(60 * 24 * 365 * 100);
    let config = config.leak();

    // The integrity hash here is placeholder text — the gate fails
    // before the tarball is fetched, so checksum verification never
    // runs and the value doesn't have to match the mock's actual
    // payload. The lockfile only needs to deserialize.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      '@pnpm.e2e/hello-world-js-bin':"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  '@pnpm.e2e/hello-world-js-bin@1.0.0':"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}"
        "snapshots:"
        "  '@pnpm.e2e/hello-world-js-bin@1.0.0': {}"
    })
    .expect("parse lockfile fixture");

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

    let err = result.expect_err("100-year cutoff must reject every entry");
    let InstallError::LockfileVerification(ref verify_err) = err else {
        panic!("expected InstallError::LockfileVerification, got {err:?}");
    };
    assert!(
        matches!(
            verify_err,
            pnpm_lockfile_verification::VerifyError::MinimumReleaseAgeViolation { .. }
        ),
        "expected MinimumReleaseAgeViolation, got {verify_err:?}",
    );

    // The gate must short-circuit before any virtual-store
    // materialization — no slot, no project-side symlink.
    let slot = dirs.project_root.join("node_modules/.pacquet/@pnpm.e2e+hello-world-js-bin@1.0.0");
    assert!(!slot.exists(), "the gate must fail before any virtual-store materialization");
    assert!(
        !dirs.project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "the gate must fail before any project-side symlinks are created",
    );

    drop((dirs.dir, mock_instance));
}
#[tokio::test]
async fn prefer_frozen_install_writes_missing_current_lockfile() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    fs::create_dir_all(&dirs.project_root).unwrap();
    let manifest_path = dirs.project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
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
        disable_optimistic_repeat_install: true,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("first install should succeed");

    let current_lockfile_path = dirs.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
    fs::remove_file(&current_lockfile_path).expect("remove current lockfile");
    let wanted = Lockfile::load_wanted_from_dir(&dirs.project_root)
        .expect("parse wanted lockfile")
        .expect("wanted lockfile should exist");

    Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&wanted)),
        lockfile_path: None,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        frozen_lockfile: false,
        prefer_frozen_lockfile: Some(true),
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
        disable_optimistic_repeat_install: true,
        pnpmfile_hook_override: None,
        workspace_projects_override: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("prefer-frozen reinstall should succeed");

    assert!(
        current_lockfile_path.is_file(),
        "prefer-frozen reinstall must restore the current lockfile",
    );

    drop((dirs.dir, mock_instance));
}
/// Dispatch state 2: no `--frozen-lockfile` flag, lockfile present and
/// fresh, `preferFrozenLockfile: true` (the default) → auto-frozen.
/// We prove the frozen path was taken the same way
/// [`warm_reinstall_skips_snapshot_when_current_lockfile_matches`] does:
/// the lockfile points at a bogus tarball URL, and the install is
/// pre-seeded with a matching current lockfile + virtual-store slot,
/// so only the snapshot-skip path inside the frozen install can
/// produce a successful run. If the dispatch silently fell through to
/// the fresh-resolve path, the bogus URL would be fetched and the
/// install would error out.
#[tokio::test]
async fn prefer_frozen_lockfile_takes_frozen_path_when_lockfile_is_fresh() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Same legacy-layout opt-out as the sibling skip test — the seed
    // helper writes the flat-name slot shape.
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    std::fs::create_dir_all(&dirs.virtual_store_dir).unwrap();
    lockfile
        .save_current_to_virtual_store_dir(&dirs.virtual_store_dir)
        .expect("seed current lockfile");
    seed_placeholder_virtual_store_slot(&dirs.virtual_store_dir);

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
        // No `--frozen-lockfile`; the dispatch must auto-go-frozen
        // via `config.prefer_frozen_lockfile` (defaults to `true`).
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        skip_runtimes: false,
        trust_lockfile: true, // fixture pins a tripwire tarball URL; skip resolution verification so the tarball-URL check doesn't flag it before the path under test
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
    .expect(
        "auto-frozen dispatch must short-circuit the bogus fetch via the skip path \
         (would otherwise error out on the invalid URL)",
    );

    drop(dirs.dir);
}
/// Dispatch state 3a: lockfile present + matching manifest, but
/// `Install::prefer_frozen_lockfile = Some(false)` (the CLI's
/// `--no-prefer-frozen-lockfile` opt-out). The dispatch must route to
/// the fresh-resolve path even though the frozen fast path would have
/// applied. We prove it by pointing at an unreachable registry: the
/// fresh-resolve path will hit the resolver and fail, whereas the
/// frozen fast path would short-circuit the network entirely via the
/// skip cache.
#[tokio::test]
async fn no_prefer_frozen_lockfile_flag_forces_fresh_resolve() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("placeholder", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    // Force the resolver onto an unreachable registry so the
    // fresh-resolve path errors out clearly; the frozen path would
    // never consult the registry at all.
    config.registry = "http://invalid.local/".to_string();
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(PARTIAL_INSTALL_LOCKFILE)
        .expect("parse partial-install fixture lockfile");

    // Seed exactly as the auto-frozen test does — if dispatch did go
    // frozen, the skip cache would carry the install to success.
    std::fs::create_dir_all(&dirs.virtual_store_dir).unwrap();
    lockfile
        .save_current_to_virtual_store_dir(&dirs.virtual_store_dir)
        .expect("seed current lockfile");
    seed_placeholder_virtual_store_slot(&dirs.virtual_store_dir);

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
        frozen_lockfile: false,
        // Opt-out at the call site (the `--no-prefer-frozen-lockfile`
        // CLI flag would land here).
        prefer_frozen_lockfile: Some(false),
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

    let err = result.expect_err(
        "fresh-resolve dispatch must consult the unreachable registry and fail; \
         a success would mean the dispatch silently took the frozen fast path",
    );
    assert!(
        !matches!(err, InstallError::OutdatedLockfile { .. }),
        "fresh-resolve fall-through must not surface as OutdatedLockfile, got {err:?}",
    );
}
/// End-to-end: when `.modules.yaml`, `<virtual_store_dir>/lock.yaml`,
/// and the wanted lockfile all agree — and the tree those files
/// describe is still on disk (the short-circuit probes it; a missing
/// entry must fall through to the repairing full path) —
/// [`Install::run`] must emit the `name: "pnpm"` "Lockfile is up to
/// date" log and return without running materialization (the
/// `allProjectsAreUpToDate` + `validateModules` short-circuit).
#[tokio::test]
async fn frozen_install_short_circuits_when_modules_and_lockfile_are_consistent() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dirs = InstallDirs::new();

    std::fs::create_dir_all(&dirs.project_root).expect("create project root");
    let manifest_path = dirs.project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // A sibling `link:` dependency keeps the lockfile non-empty without
    // requiring registry fetches — the gate fires on the eligibility
    // checks alone, materialization is never reached so the
    // (non-existent) link target doesn't matter.
    manifest.add_dependency("sibling", "link:../sibling", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      sibling:"
        "        specifier: link:../sibling"
        "        version: link:../sibling"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse minimal v9 lockfile with one link dep");

    let included = pnpm_modules_yaml::IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };

    // Seed the on-disk state a previous install would have left.
    let seed_modules = Modules {
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
    write_modules_manifest::<Host>(&dirs.modules_dir, seed_modules).expect("seed .modules.yaml");
    lockfile
        .save_current_to_virtual_store_dir(&dirs.virtual_store_dir)
        .expect("seed current lockfile");
    // Date the manifest inside a millisecond after the lockfile, as a
    // sub-millisecond filesystem leaves a manifest edited after the last
    // lockfile write: the manifest's truncated mtime alone would read as
    // modified against itself on the next `pnpm run`.
    let validated_at = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    pnpm_testing_utils::fs::set_mtime(
        &dirs.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME),
        validated_at,
    );
    pnpm_testing_utils::fs::set_mtime(manifest.path(), validated_at + Duration::from_micros(500));
    // The short-circuit probes the tree it would skip; seed the one
    // direct-dep entry the lockfile records so the probe holds. A
    // marker *file* doubles as the materialization sentinel below:
    // the full pipeline's link pass would displace it with a symlink,
    // so its untouched survival proves the gate fired.
    std::fs::write(dirs.modules_dir.join("sibling"), b"sentinel: not a symlink")
        .expect("seed the sibling link entry");

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
        trust_lockfile: true,
        update_checksums: false,
        mutation: ProjectMutation::InstallWorkspace,
        installs_only: true,
        supported_architectures: None,
        node_linker: pnpm_config::NodeLinker::Isolated,
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
    .run::<RecordingReporter>()
    .await
    .expect("up-to-date install should succeed via the short-circuit");

    let captured = EVENTS.lock().unwrap();
    assert!(
        captured.iter().any(|event| matches!(
            event,
            LogEvent::Pnpm(log)
                if log.message == "Lockfile is up to date, resolution step is skipped"
        )),
        r#"the `name: "pnpm"` up-to-date log must be emitted when the install short-circuits"#,
    );

    assert!(
        captured.iter().any(|e| matches!(e, LogEvent::Stage(s) if s.stage == Stage::ImportingDone)),
        "ImportingDone must close the importing bracket on the fast path",
    );
    assert!(
        captured.iter().any(|e| matches!(e, LogEvent::Summary(_))),
        "Summary must fire so `pnpm:root` history renders even on the fast path",
    );

    // Materialization on the regular install path replaces the seeded
    // marker file with a symlink (displacing the squatter), so the
    // marker surviving as a plain file proves the gate skipped the
    // link pass.
    let sibling_link = dirs.modules_dir.join("sibling");
    let sibling_meta =
        std::fs::symlink_metadata(&sibling_link).expect("the seeded sibling entry must survive");
    assert!(
        sibling_meta.file_type().is_file(),
        "the link: dep must NOT be materialized when the gate fires; \
         a symlink at {sibling_link:?} would mean the install ran the full pipeline",
    );

    // Workspace state is still refreshed so the next `pnpm run`'s
    // `verifyDepsBeforeRun` doesn't fire spuriously.
    let written = load_workspace_state(&dirs.project_root)
        .expect("read workspace state")
        .expect("workspace state must be written");
    let manifest_mtime = crate::optimistic_repeat_install::file_mtime(manifest.path())
        .expect("manifest should have an mtime");
    assert_eq!(
        manifest_mtime.ns, 1_700_000_000_000_500_000,
        "the short-circuit must leave the manifest untouched",
    );
    assert!(
        !crate::optimistic_repeat_install::modified_at_or_after(
            manifest_mtime,
            written.last_validated_timestamp,
        ),
        "the refreshed state must cover the validated manifest mtime; got {}",
        written.last_validated_timestamp,
    );

    drop(dirs.dir);
}
