use super::{
    super::{Install, InstallError, ProjectMutation},
    InstallDirs,
};
use crate::PolicyExcludes;
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_modules_yaml::{
    DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH, Host, LayoutVersion, Modules, NodeLinker,
    read_modules_manifest, write_modules_manifest,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use tempfile::tempdir;
use text_block_macros::text_block;

/// GVS-off frozen-lockfile install. The dispatch path is the same,
/// but `Install::run` skips the project-registry write entirely.
/// Pins that turning off `enable_global_virtual_store` makes the
/// install behave like today — no `<store_dir>/projects/` directory
/// appears.
#[tokio::test]
async fn frozen_lockfile_with_gvs_off_skips_project_registry() {
    let dirs = InstallDirs::new();

    std::fs::create_dir_all(&dirs.project_root).expect("create project root");
    let manifest_path = dirs.project_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.enable_global_virtual_store = false;
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
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
    .expect("frozen-lockfile install with GVS off should succeed");

    assert!(
        !dirs.store_dir.join("v11/projects").exists(),
        "GVS-off install must NOT create the project-registry directory",
    );

    drop(dirs.dir);
}
/// Workspace install under GVS registers the workspace root once,
/// regardless of how many importers the workspace declares. The
/// project registration fires exactly once per install against the
/// workspace root — store prune walks
/// `<workspace>/node_modules/.pnpm/` to find every installed package,
/// so one registry entry per workspace is enough.
#[tokio::test]
async fn frozen_lockfile_under_gvs_registers_workspace_root_only() {
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let workspace_root = dir.path().join("workspace");
    let modules_dir = workspace_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    // Workspace layout: root + one sub-importer. The sub-importer's
    // directory exists on disk because the lockfile reader needs it,
    // but registration only resolves the workspace root.
    let web_dir = workspace_root.join("packages/web");
    std::fs::create_dir_all(&web_dir).expect("create packages/web");
    let manifest_path = workspace_root.join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    std::fs::write(web_dir.join("package.json"), "{}").expect("write packages/web/package.json");

    let mut config = Config::new();
    config.enable_global_virtual_store = true;
    config.lockfile = false;
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    config.global_virtual_store_dir = store_dir.join("links");
    let config = config.leak();

    // Two importers: `.` and `packages/web`. Empty dep graph so the
    // install reaches the registry-write call without doing any actual
    // fetch/link work.
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies: {}"
        "  packages/web:"
        "    dependencies: {}"
        "packages: {}"
        "snapshots: {}"
    })
    .expect("parse minimal v9 workspace lockfile");

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
    .expect("workspace frozen-lockfile install under GVS should succeed");

    // Exactly one registry entry resolving back to the workspace
    // root, matching pnpm's once-per-install `registerProject(storeDir,
    // lockfileDir)` shape.
    let projects_dir = store_dir.join("v11/projects");
    assert!(
        projects_dir.is_dir(),
        "GVS-on workspace install must create <store_dir>/v11/projects/",
    );
    let entries: Vec<_> =
        std::fs::read_dir(&projects_dir).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(
        entries.len(),
        1,
        "workspace install registers the workspace root once, not once per importer",
    );
    assert_eq!(
        dunce::canonicalize(entries[0].path()).expect("canonicalize registry entry"),
        dunce::canonicalize(&workspace_root).expect("canonicalize workspace root"),
        "registry symlink must resolve back to the workspace root",
    );

    drop(dir);
}
/// End-to-end read → seed → write loop. Pre-write
/// `.modules.yaml.skipped` before a frozen-lockfile install runs;
/// confirm the install picks up the seed and re-writes the file
/// with the same key. The lockfile here has empty `snapshots: {}` so
/// the constraint-free fast path runs — that's the branch in
/// [`InstallFrozenLockfile::run`] that preserves the seed verbatim
/// without calling `compute_skipped_snapshots`. Together with the
/// unit tests on the slow path, this pins the full plumbing between
/// `read_modules_manifest`, `compute_skipped_snapshots`'s seed
/// arg, the threading out of [`InstallFrozenLockfileOutput`], and
/// `build_modules_manifest`'s serialization.
///
/// [`InstallFrozenLockfile::run`]: super::super::super::super::InstallFrozenLockfile::run
/// [`InstallFrozenLockfileOutput`]: super::super::super::super::InstallFrozenLockfileOutput
#[tokio::test]
async fn frozen_install_preserves_seeded_skipped_across_reinstall() {
    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(manifest_path).unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    // Pre-write `.modules.yaml` with a non-empty `skipped` list —
    // models the state left by a previous install that landed
    // platform-mismatched optional deps. Two entries: one bare
    // package and one scoped, so the parse-then-serialize round-trip
    // covers both `name@version` and `@scope/name@version`.
    let seeded_keys = ["previously-skipped@1.0.0", "@scope/also-skipped@2.3.4"];
    let seed_modules = Modules {
        layout_version: Some(LayoutVersion),
        node_linker: Some(NodeLinker::Isolated),
        store_dir: dirs.store_dir.display().to_string(),
        virtual_store_dir: dirs.virtual_store_dir.to_string_lossy().into_owned(),
        virtual_store_dir_max_length: DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH,
        skipped: seeded_keys.iter().map(|s| (*s).to_string()).collect(),
        ..Default::default()
    };
    write_modules_manifest::<Host>(&dirs.modules_dir, seed_modules).expect("seed .modules.yaml");

    // Empty lockfile drives the constraint-free fast path. The
    // seed must survive verbatim.
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
    .expect("frozen-lockfile install should succeed");

    let written = dirs
        .modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");

    // Both seed entries survive — proves the read → seed → write
    // loop is fully wired. Order is the sort-on-write order:
    // `@scope/...` < `previously-...` lexically, so the scoped
    // entry leads.
    assert_eq!(written.skipped.len(), 2, "both seeded entries must survive");
    assert!(written.skipped.contains(&"previously-skipped@1.0.0".to_string()));
    assert!(written.skipped.contains(&"@scope/also-skipped@2.3.4".to_string()));
    let sorted: Vec<&str> = written.skipped.iter().map(String::as_str).collect();
    assert_eq!(
        sorted,
        ["@scope/also-skipped@2.3.4", "previously-skipped@1.0.0"],
        "write_modules_manifest must sort the list alphabetically",
    );

    drop(dirs.dir);
}
/// Scenario: skipping an optional dependency if it cannot be fetched.
/// An `optional: true` snapshot whose tarball URL is unreachable must
/// not abort the install — the failure is silently swallowed at
/// the per-snapshot fetch dispatch in `CreateVirtualStore`.
///
/// Asserts:
/// 1. The install resolves `Ok` (no abort).
/// 2. The broken snapshot's virtual-store slot was NOT created.
/// 3. The on-disk `.modules.yaml.skipped` does NOT contain the
///    broken snapshot — fetch failures are transient (the catch site
///    never updates `opts.skipped`), so a subsequent install retries
///    the fetch.
#[tokio::test]
async fn frozen_install_silently_swallows_unreachable_optional_tarball() {
    // Lockfile with one `optional: true` snapshot whose `tarball` URL
    // dials `127.0.0.1:1` (a reserved port that always refuses) so
    // the fetch reliably fails without a network round-trip. The
    // integrity is arbitrary — we never get far enough to verify it.
    const BROKEN_OPTIONAL_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    optionalDependencies:"
        "      broken-pkg:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  broken-pkg@1.0.0:"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA, tarball: 'http://127.0.0.1:1/broken.tgz'}"
        "snapshots:"
        "  broken-pkg@1.0.0:"
        "    optional: true"
    };

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest must match the lockfile importer entry so the
    // freshness check (<https://github.com/pnpm/pacquet/issues/447>) doesn't reject the install before we
    // reach the fetch site.
    manifest.add_dependency("broken-pkg", "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    // Opt out of the GVS layout so the assertion below can stat the
    // legacy `<dirs.virtual_store_dir>/<flat-name>` slot directly. With
    // GVS on, the slot lives under `<dirs.store_dir>/links/...` and the
    // assertion would always pass regardless of whether the swallow
    // path actually fired.
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    // Keep retries minimal — 127.0.0.1:1 fails immediately on every
    // try, but a long retry schedule would dominate the test runtime.
    config.fetch_retries = 0;
    config.minimum_release_age = None;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(BROKEN_OPTIONAL_LOCKFILE)
        .expect("parse broken-optional fixture lockfile");

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
        skip_runtimes: false,
        // The lockfile-resolution verifier is unrelated to what this test
        // exercises (the optional-tarball swallow path) and now always runs;
        // its fail-closed tarball-URL check would otherwise try to fetch
        // metadata for `broken-pkg` from the unreachable default registry and
        // abort the install before the optional-snapshot code path runs.
        // `trust_lockfile` is the opt-out that skips verification entirely.
        trust_lockfile: true,
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
    .expect("install must NOT abort when an optional snapshot fails to fetch");

    // The broken snapshot's virtual-store slot must not have been
    // created — the cold-batch dispatch failed before extraction.
    let expected_slot = dirs.virtual_store_dir.join("broken-pkg@1.0.0").join("node_modules");
    assert!(
        !expected_slot.exists(),
        "broken optional snapshot's slot must not exist, found {expected_slot:?}",
    );

    // The fetch-failure entry must NOT have been persisted to
    // `.modules.yaml.skipped`. The silent catch site never updates
    // `opts.skipped`, so a future install retries the fetch (in case
    // the URL becomes reachable again).
    let written = dirs
        .modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");
    assert!(
        written.skipped.is_empty(),
        "fetch-failure entries must not land in .modules.yaml.skipped, got {:?}",
        written.skipped,
    );

    drop(dirs.dir);
}
/// The fetch-failure swallow is gated on `snapshot.optional`. A
/// non-optional snapshot whose tarball is unreachable must still
/// abort the install (the swallow is `if optional return; else throw`).
/// Same fixture as the swallow test but with `optional: true`
/// removed from the snapshot entry — confirms the polarity is
/// correct.
#[tokio::test]
async fn frozen_install_propagates_non_optional_fetch_failure() {
    const NON_OPTIONAL_BROKEN_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      broken-pkg:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages:"
        "  broken-pkg@1.0.0:"
        "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA, tarball: 'http://127.0.0.1:1/broken.tgz'}"
        "snapshots:"
        "  broken-pkg@1.0.0: {}"
    };

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("broken-pkg", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    // Match the sister test's GVS-off setup so both swallow tests
    // route through the same layout — sidesteps any GVS-routing
    // path divergence affecting where the cold-batch dispatch even
    // runs.
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.fetch_retries = 0;
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(NON_OPTIONAL_BROKEN_LOCKFILE)
        .expect("parse non-optional broken-fixture lockfile");

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

    assert!(result.is_err(), "non-optional fetch failure must abort the install, got {result:?}");

    drop(dirs.dir);
}
/// The frozen-install `--no-optional` scenario.
///
/// The fixture is designed to discriminate slice 5 (`--no-optional`
/// filter) from slice 4 (fetch-failure swallow): a snapshot with
/// `optional: true` whose **metadata row is missing from
/// `packages:`**. Slice 4's swallow only covers `DownloadTarball`
/// and `GitFetch` — `MissingPackageMetadata` propagates even for
/// optional snapshots. So if slice 5's filter doesn't fire, the
/// missing-metadata error aborts the install regardless of slice
/// 4. A successful install therefore proves the snapshot was
/// dropped **before** cache-key derivation by the slice 5 gate.
///
/// When `!include.optionalDependencies`, every depNode whose
/// `optional` flag is true is dropped from the install graph
/// before extraction / linking / building runs.
///
/// Also asserts that `--no-optional` exclusions are **not**
/// persisted to `.modules.yaml.skipped` — same convention as the
/// fetch-failure swallow (slice 4): the exclusion is transient,
/// so a later install without `--no-optional` brings the snapshot
/// back into the install graph.
#[tokio::test]
async fn frozen_install_no_optional_drops_optional_only_snapshots() {
    // Lockfile with one `optional: true` snapshot whose metadata
    // row is intentionally missing from `packages:`. Slice 4
    // (fetch-failure swallow) does NOT cover `MissingPackageMetadata`,
    // so reaching the cache-key derivation step would abort the
    // install. The `--no-optional` filter must drop the snapshot
    // before that step runs.
    const OPTIONAL_NO_METADATA_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    optionalDependencies:"
        "      drop-me:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages: {}"
        "snapshots:"
        "  drop-me@1.0.0:"
        "    optional: true"
    };

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    // Manifest matches the lockfile importer entry so the
    // freshness check doesn't reject the install.
    manifest.add_dependency("drop-me", "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    // Opt out of GVS so the slot-path assertion targets the legacy
    // `<dirs.virtual_store_dir>/<flat-name>` layout. Same pattern as the
    // slice 4 swallow tests.
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(OPTIONAL_NO_METADATA_LOCKFILE)
        .expect("parse optional-no-metadata fixture lockfile");

    // The dispatch list excludes `DependencyGroup::Optional` — same
    // shape `--no-optional` produces from
    // `InstallDependencyOptions::dependency_groups()` in
    // `crates/cli/src/cli_args/install.rs`.
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
    .expect("install must succeed with --no-optional despite missing optional metadata");

    let expected_slot = dirs.virtual_store_dir.join("drop-me@1.0.0").join("node_modules");
    assert!(
        !expected_slot.exists(),
        "optional-only snapshot's slot must not exist, found {expected_slot:?}",
    );

    // Transient — must not bleed into the persistent
    // `.modules.yaml.skipped` set.
    let written = dirs
        .modules_dir
        .pipe_as_ref(read_modules_manifest::<Host>)
        .expect("read .modules.yaml")
        .expect("modules manifest exists");
    assert!(
        written.skipped.is_empty(),
        "--no-optional exclusions must not land in .modules.yaml.skipped, got {:?}",
        written.skipped,
    );

    drop(dirs.dir);
}
/// Polarity test for [`frozen_install_no_optional_drops_optional_only_snapshots`].
/// Same fixture, but the dispatch list **includes** `Optional`. With
/// the snapshot's metadata missing from `packages:`, the install
/// must now abort with `MissingPackageMetadata` — slice 4's
/// fetch-failure swallow doesn't cover that variant, so the
/// optional-ness alone doesn't save the install. Proves the
/// slice 5 filter is gated on the dispatch list rather than firing
/// unconditionally.
#[tokio::test]
async fn frozen_install_optional_included_surfaces_missing_metadata() {
    const OPTIONAL_NO_METADATA_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    optionalDependencies:"
        "      drop-me:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages: {}"
        "snapshots:"
        "  drop-me@1.0.0:"
        "    optional: true"
    };

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("drop-me", "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(OPTIONAL_NO_METADATA_LOCKFILE)
        .expect("parse optional-no-metadata fixture lockfile");

    let result = Install {
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

    let err =
        result.expect_err("install must abort without --no-optional when metadata is missing");
    assert!(
        matches!(
            err,
            InstallError::FrozenLockfile(crate::InstallFrozenLockfileError::CreateVirtualStore(
                crate::CreateVirtualStoreError::MissingPackageMetadata { .. },
            ),),
        ),
        "expected FrozenLockfile(CreateVirtualStore(MissingPackageMetadata)), got {err:?}",
    );

    drop(dirs.dir);
}
/// Regression coverage for the shared-dependency case
/// (`dependency that is both optional and non-optional is installed,
/// when optional dependencies should be skipped`).
///
/// `SnapshotEntry::optional` is set by the resolver only
/// when a snapshot is reachable **exclusively** through optional
/// edges. A snapshot reachable through any non-optional edge carries
/// `optional: false` and **must not** be dropped by `--no-optional`.
///
/// Fixture: a single snapshot `shared@1.0.0` with `optional: false`
/// (default) and metadata missing from `packages:`. With
/// `--no-optional`, the filter must skip this snapshot only if it
/// checks the `optional` flag — if it accidentally drops every
/// snapshot listed under `optionalDependencies` regardless of the
/// flag, the install would silently succeed (the missing-metadata
/// error wouldn't surface). Conversely, if the filter is correct,
/// the install aborts with `MissingPackageMetadata` because the
/// non-optional snapshot reaches cache-key derivation.
#[tokio::test]
async fn frozen_install_no_optional_keeps_shared_non_optional_snapshot() {
    const SHARED_NON_OPTIONAL_LOCKFILE: &str = text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    optionalDependencies:"
        "      shared:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "packages: {}"
        "snapshots:"
        "  shared@1.0.0: {}"
    };

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("shared", "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.lockfile = false;
    config.enable_global_virtual_store = false;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    let config = config.leak();

    let lockfile: Lockfile = serde_saphyr::from_str(SHARED_NON_OPTIONAL_LOCKFILE)
        .expect("parse shared-non-optional fixture lockfile");

    let result = Install {
        tarball_mem_cache: Default::default(),
        http_client: &Default::default(),
        http_client_arc: std::sync::Arc::new(Default::default()),
        config,
        manifest: &manifest,
        emit_initial_manifest: true,
        lockfile: MaybeLazyLockfile::Loaded(Some(&lockfile)),
        lockfile_path: None,
        // `--no-optional` shape: Optional NOT in the dispatch list.
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

    let err =
        result.expect_err("snapshot with optional:false must NOT be dropped by --no-optional");
    assert!(
        matches!(
            err,
            InstallError::FrozenLockfile(crate::InstallFrozenLockfileError::CreateVirtualStore(
                crate::CreateVirtualStoreError::MissingPackageMetadata { .. },
            ),),
        ),
        "expected FrozenLockfile(CreateVirtualStore(MissingPackageMetadata)) — \
         proves the snapshot was kept; got {err:?}",
    );

    drop(dirs.dir);
}
