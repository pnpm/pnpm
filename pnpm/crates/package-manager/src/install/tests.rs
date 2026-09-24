#![expect(
    clippy::default_trait_access,
    reason = "struct-literal test fixtures; field types are evident from the literal and naming each would force ~20 imports"
)]

mod catalogs;

mod store;

mod frozen_policy;

mod frozen_dispatch;

mod frozen_optional;

mod frozen_freshness;

mod lockfile_recovery;

mod lockfile_writes;

mod runtimes;

mod resolution;

mod reporting;

mod repeat_install;

mod fresh_outputs;

mod install_outputs;

mod links;

mod builds;

mod workspace;

mod hooks;

use super::{Install, InstallError, ProjectMutation};
use crate::{
    PolicyExcludes, install::apply_materialization::completion::report_verified_file_integrity,
};
use pnpm_config::Config;
use pnpm_lockfile::{ComVer, Lockfile, LockfileVersion, MaybeLazyLockfile};
use pnpm_modules_yaml::Host;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{HookLog, LogEvent, LogLevel, Reporter, SilentReporter};
use pnpm_store_dir::VerifiedFileIntegrity;
use pnpm_testing_utils::registry::TestRegistry;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use tempfile::{TempDir, tempdir};
use text_block_macros::text_block;

/// The temp directories one install test runs against: a store beside a
/// project with its `node_modules` and virtual store.
struct InstallDirs {
    dir: TempDir,
    store_dir: PathBuf,
    project_root: PathBuf,
    modules_dir: PathBuf,
    virtual_store_dir: PathBuf,
}

impl InstallDirs {
    fn new() -> Self {
        let dir = tempdir().unwrap();
        let store_dir = dir.path().join("pacquet-store");
        let project_root = dir.path().join("project");
        let modules_dir = project_root.join("node_modules");
        let virtual_store_dir = modules_dir.join(".pacquet");
        Self { dir, store_dir, project_root, modules_dir, virtual_store_dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }
}

fn empty_test_lockfile() -> Lockfile {
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor: 0 }).unwrap(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: std::collections::HashMap::new(),
        packages: None,
        snapshots: None,
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

/// Reading wrapper over [`super::modules_consistent_with`] for the tests
/// that exercise the `.modules.yaml`-absent and drift cases. Production
/// code reads the manifest once itself and calls `modules_consistent_with`
/// directly, so this wrapper lives with the tests.
fn is_modules_yaml_consistent(
    modules_dir: &std::path::Path,
    config: &Config,
    node_linker: pnpm_config::NodeLinker,
    included: pnpm_modules_yaml::IncludedDependencies,
) -> bool {
    pnpm_modules_yaml::read_modules_layout::<Host>(modules_dir)
        .ok()
        .flatten()
        .is_some_and(|modules| {
            super::modules_consistent_with(&modules, config, node_linker, included)
        })
}

/// Reading wrapper over [`super::modules_layout_consistent_with`] — the
/// purge-gate consistency check, which (unlike [`is_modules_yaml_consistent`])
/// ignores `included`.
fn is_modules_yaml_layout_consistent(
    modules_dir: &std::path::Path,
    config: &Config,
    node_linker: pnpm_config::NodeLinker,
) -> bool {
    pnpm_modules_yaml::read_modules_layout::<Host>(modules_dir)
        .ok()
        .flatten()
        .is_some_and(|modules| super::modules_layout_consistent_with(&modules, config, node_linker))
}

const SCOPED_TEST_INTEGRITY: &str = "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==";

fn scoped_package_body(registry_url: &str) -> String {
    format!(
        r#"{{
  "name": "@private/foo",
  "dist-tags": {{ "latest": "1.0.0" }},
  "versions": {{
    "1.0.0": {{
      "name": "@private/foo",
      "version": "1.0.0",
      "dist": {{
        "integrity": "{SCOPED_TEST_INTEGRITY}",
        "tarball": "{registry_url}@private/foo/-/foo-1.0.0.tgz"
      }}
    }}
  }}
}}"#,
    )
}

/// Unit tests for [`super::build_projects_map`] / [`super::build_workspace_state`].
/// The `projects` map must contain one entry per project in the list,
/// keyed on the project root dir. Pacquet's `build_projects_map`
/// derives the list directly from `project_manifests`, so a fresh
/// install that hasn't written a `pnpm-lock.yaml` yet still records
/// every workspace project, not just the root.
mod build_workspace_state_tests;

/// A v9 lockfile fixture pinned to a placeholder package whose
/// integrity is bogus on purpose. Pacquet enforces tarball integrity
/// on the install path, so any test that lets the install reach the
/// fetch site would fail — meaning a successful install with this
/// fixture is *proof* that the per-snapshot skip path (issue [#433]
/// section B) short-circuited the fetch entirely.
///
/// [#433]: https://github.com/pnpm/pacquet/issues/433
const PARTIAL_INSTALL_LOCKFILE: &str = text_block! {
    "lockfileVersion: '9.0'"
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
};

/// Pre-populate the virtual-store slot that [`PARTIAL_INSTALL_LOCKFILE`]
/// describes so the skip path has a directory to point at. Just the
/// `<virtual_store_dir>/placeholder@1.0.0/node_modules/placeholder`
/// dirent is enough — the skip check only stats the directory, it
/// doesn't read CAS contents.
fn seed_placeholder_virtual_store_slot(virtual_store_dir: &std::path::Path) {
    let slot = virtual_store_dir
        .join("placeholder@1.0.0")
        .join("node_modules")
        .join("placeholder");
    std::fs::create_dir_all(&slot).expect("create placeholder virtual-store slot");
}

/// Run one install against `modules_dir` for the purge regression below: a
/// fresh leaked config per call (the install path needs `&'static Config`),
/// `included` driven by `dependency_groups`, and `virtual_store_dir_max_length`
/// surfaced so the test can force a layout drift. `disable_optimistic_repeat_install`
/// keeps every call on the full path so the purge branch is actually evaluated.
async fn run_purge_regression_install(
    dirs: &InstallDirs,
    registry: &str,
    manifest: &PackageManifest,
    dependency_groups: Vec<DependencyGroup>,
    virtual_store_dir_max_length: u64,
) {
    run_purge_regression_install_with_lockfile(
        dirs,
        registry,
        manifest,
        dependency_groups,
        virtual_store_dir_max_length,
        true,
    )
    .await;
}

async fn run_purge_regression_install_with_lockfile(
    dirs: &InstallDirs,
    registry: &str,
    manifest: &PackageManifest,
    dependency_groups: Vec<DependencyGroup>,
    virtual_store_dir_max_length: u64,
    lockfile: bool,
) {
    let mut config = Config::new();
    config.lockfile = lockfile;
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.virtual_store_dir = dirs.virtual_store_dir.clone();
    config.registry = registry.to_string();
    config.virtual_store_dir_max_length = virtual_store_dir_max_length;
    let config = config.leak();
    let mutation = if dependency_groups.contains(&DependencyGroup::Dev) {
        ProjectMutation::InstallWorkspace
    } else {
        ProjectMutation::InstallSome
    };
    // One client behind both fields, matching the CLI's single-source
    // wiring documented on [`Install::http_client_arc`].
    let http_client_arc: std::sync::Arc<pnpm_network::ThrottledClient> = std::sync::Arc::default();
    Install {
        lockfile_policy: crate::InstallLockfilePolicy {
            frozen: false,
            prefer_frozen: None,
            ignore_manifest_check: false,
            trust: false,
            update_checksums: false,
            excludes: PolicyExcludes::Persist,
            disable_optimistic_repeat: true,
            manifest_freshness: crate::ManifestFreshness::Mtime,
        },
        execution: crate::InstallExecution {
            skip_runtimes: false,
            mutation,
            installs_only: true,
            node_linker: pnpm_config::NodeLinker::default(),
            lockfile_only: false,
            dry_run: false,
        },
        resolution: crate::ResolutionInputs {
            update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
        },
        context: crate::InstallInvocation {
            http_client: &http_client_arc,
            config,
            manifest,
            emit_initial_manifest: true,
            lockfile: MaybeLazyLockfile::Loaded(None),
            lockfile_path: None,
        },
        fetching: crate::InstallFetching {
            tarball_mem_cache: Default::default(),
            http_client_arc: std::sync::Arc::clone(&http_client_arc),
            resolved_packages: &Default::default(),
        },
        projects: crate::InstallProjects {
            dependency_groups,
            supported_architectures: None,
            catalogs_override: None,
            pnpmfile_hook_override: None,
            policy_excludes_dir: None,
            workspace_projects_override: None,
        },
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");
}

/// Shared setup for the offline repeat-install regression tests below:
/// a real install against the mock registry, after which the registry
/// is dropped and the packument cache is wiped. Any code path that
/// falls off the optimistic fast path — the resolver, the
/// lockfile-verification fan-out, a tarball fetch — would have to
/// reach the dead `127.0.0.1:9` registry and fail the install, so the
/// `expect` on the second run is the regression tripwire for the
/// repeat-install optimizations (the benchmarks don't run in CI; these
/// tests are what pins the "zero network, zero pipeline" property).
async fn install_then_go_offline() -> (tempfile::TempDir, &'static Config, PackageManifest) {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let store_dir = dir.path().join("pacquet-store");
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    std::fs::create_dir_all(&project_root).expect("create project root");
    let manifest_path = project_root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest
        .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
        .unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.cache_dir = cache_dir.clone();
    config.store_dir = store_dir.clone().into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    config.registry = mock_instance.url().to_string();
    let config = config.leak();

    Install {
        lockfile_policy: crate::InstallLockfilePolicy {
            frozen: false,
            prefer_frozen: None,
            ignore_manifest_check: false,
            trust: false,
            update_checksums: false,
            excludes: PolicyExcludes::Persist,
            disable_optimistic_repeat: false,
            manifest_freshness: crate::ManifestFreshness::Mtime,
        },
        execution: crate::InstallExecution {
            skip_runtimes: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            node_linker: pnpm_config::NodeLinker::default(),
            lockfile_only: false,
            dry_run: false,
        },
        resolution: crate::ResolutionInputs {
            update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
        },
        context: crate::InstallInvocation {
            http_client: &Default::default(),
            config,
            manifest: &manifest,
            emit_initial_manifest: true,
            lockfile: MaybeLazyLockfile::Loaded(None),
            lockfile_path: None,
        },
        fetching: crate::InstallFetching {
            tarball_mem_cache: Default::default(),
            http_client_arc: std::sync::Arc::new(Default::default()),
            resolved_packages: &Default::default(),
        },
        projects: crate::InstallProjects {
            dependency_groups: [DependencyGroup::Prod],
            supported_architectures: None,
            catalogs_override: None,
            pnpmfile_hook_override: None,
            policy_excludes_dir: None,
            workspace_projects_override: None,
        },
    }
    .run::<SilentReporter>()
    .await
    .expect("first install must succeed");

    drop(mock_instance);
    // The benchmark harness wipes `~/.cache/pnpm` (packument cache +
    // `lockfile-verified.jsonl`) before every run; do the same so a
    // regression can't hide behind a cache hit.
    std::fs::remove_dir_all(&cache_dir).expect("wipe the cache dir");

    let mut offline_config = Config::new();
    offline_config.cache_dir = cache_dir;
    offline_config.store_dir = store_dir.into();
    offline_config.modules_dir = modules_dir;
    offline_config.virtual_store_dir = virtual_store_dir;
    offline_config.registry = "http://127.0.0.1:9/".to_string();
    let offline_config = offline_config.leak();

    (dir, offline_config, manifest)
}

/// Rewrite `package.json` with identical content but a strictly newer
/// mtime — the shape the vlt.sh benchmark prepare step (`npm pkg
/// delete`, `touch`) produces before every timed run.
fn touch_manifest(manifest: &PackageManifest) -> PackageManifest {
    let manifest_path = manifest.path().to_path_buf();
    let manifest_text = std::fs::read_to_string(&manifest_path).expect("read package.json");
    std::fs::write(&manifest_path, manifest_text).expect("refresh package.json mtime");
    pnpm_testing_utils::fs::bump_mtime(&manifest_path);
    PackageManifest::from_path(manifest_path).expect("reload manifest")
}

async fn fresh_lockfile_only_with_overrides(
    dependencies: &[(&str, &str)],
    overrides: &[(&str, &str)],
    workspace_yaml: Option<&str>,
) -> (tempfile::TempDir, Lockfile) {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    if let Some(workspace_yaml) = workspace_yaml {
        std::fs::write(dir.path().join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    }
    let store_dir = dir.path().join("pacquet-store");
    let modules_dir = dir.path().join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    for (name, spec) in dependencies {
        manifest.add_dependency(name, spec, DependencyGroup::Prod).unwrap();
    }
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url().to_string();
    if !overrides.is_empty() {
        let mut map = indexmap::IndexMap::new();
        for (selector, spec) in overrides {
            map.insert((*selector).to_string(), (*spec).to_string());
        }
        config.overrides = Some(map);
    }
    let config = config.leak();

    Install {
        lockfile_policy: crate::InstallLockfilePolicy {
            frozen: false,
            prefer_frozen: Some(false),
            ignore_manifest_check: false,
            trust: false,
            update_checksums: false,
            excludes: PolicyExcludes::Persist,
            disable_optimistic_repeat: false,
            manifest_freshness: crate::ManifestFreshness::Mtime,
        },
        execution: crate::InstallExecution {
            skip_runtimes: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            node_linker: pnpm_config::NodeLinker::default(),
            lockfile_only: true,
            dry_run: false,
        },
        resolution: crate::ResolutionInputs {
            update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
        },
        context: crate::InstallInvocation {
            http_client: &Default::default(),
            config,
            manifest: &manifest,
            emit_initial_manifest: true,
            lockfile: MaybeLazyLockfile::Loaded(None),
            lockfile_path: None,
        },
        fetching: crate::InstallFetching {
            tarball_mem_cache: Default::default(),
            http_client_arc: std::sync::Arc::new(Default::default()),
            resolved_packages: &Default::default(),
        },
        projects: crate::InstallProjects {
            dependency_groups: [DependencyGroup::Prod],
            supported_architectures: None,
            catalogs_override: None,
            pnpmfile_hook_override: None,
            policy_excludes_dir: None,
            workspace_projects_override: None,
        },
    }
    .run::<SilentReporter>()
    .await
    .expect("lockfile-only install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(lockfile_path).expect("read lockfile");
    let lockfile = serde_saphyr::from_str(&content).expect("parse lockfile");
    (dir, lockfile)
}

fn assert_package_present(lockfile: &Lockfile, key: &str) {
    let key: pnpm_lockfile::PackageKey = key.parse().unwrap();
    assert!(
        lockfile.packages
            .as_ref()
            .is_some_and(|packages| packages.contains_key(&key)),
        "expected packages to contain {key}",
    );
}

fn assert_package_absent(lockfile: &Lockfile, key: &str) {
    let key: pnpm_lockfile::PackageKey = key.parse().unwrap();
    assert!(
        lockfile.packages
            .as_ref()
            .is_none_or(|packages| !packages.contains_key(&key)),
        "expected packages not to contain {key}",
    );
}

async fn fresh_lockfile_only_with_compatibility_db(
    ignore_compatibility_db: bool,
) -> (tempfile::TempDir, Lockfile) {
    let mock_instance = TestRegistry::start();

    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("pacquet-store");
    let modules_dir = dir.path().join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    manifest.add_dependency("debug", "4.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = mock_instance.url().to_string();
    config.ignore_compatibility_db = ignore_compatibility_db;
    let config = config.leak();

    Install {
        lockfile_policy: crate::InstallLockfilePolicy {
            frozen: false,
            prefer_frozen: Some(false),
            ignore_manifest_check: false,
            trust: false,
            update_checksums: false,
            excludes: PolicyExcludes::Persist,
            disable_optimistic_repeat: false,
            manifest_freshness: crate::ManifestFreshness::Mtime,
        },
        execution: crate::InstallExecution {
            skip_runtimes: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            node_linker: pnpm_config::NodeLinker::default(),
            lockfile_only: true,
            dry_run: false,
        },
        resolution: crate::ResolutionInputs {
            update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
        },
        context: crate::InstallInvocation {
            http_client: &Default::default(),
            config,
            manifest: &manifest,
            emit_initial_manifest: true,
            lockfile: MaybeLazyLockfile::Loaded(None),
            lockfile_path: None,
        },
        fetching: crate::InstallFetching {
            tarball_mem_cache: Default::default(),
            http_client_arc: std::sync::Arc::new(Default::default()),
            resolved_packages: &Default::default(),
        },
        projects: crate::InstallProjects {
            dependency_groups: [DependencyGroup::Prod],
            supported_architectures: None,
            catalogs_override: None,
            pnpmfile_hook_override: None,
            policy_excludes_dir: None,
            workspace_projects_override: None,
        },
    }
    .run::<SilentReporter>()
    .await
    .expect("lockfile-only install should succeed");

    let lockfile_path = dir.path().join(Lockfile::FILE_NAME);
    let content = std::fs::read_to_string(lockfile_path).expect("read lockfile");
    let lockfile = serde_saphyr::from_str(&content).expect("parse lockfile");
    (dir, lockfile)
}

/// Runs a fresh install in `root` with `root_deps` as direct prod
/// dependencies and `pnpmfile_src` written to `<root>/.pnpmfile.cjs`, so the
/// pnpmfile hooks are discovered and run during resolution.
async fn install_with_pnpmfile(
    registry_url: &str,
    root: &std::path::Path,
    root_deps: &[(&str, &str)],
    pnpmfile_src: &str,
) -> Result<(), InstallError> {
    install_with_pnpmfile_reporter::<SilentReporter>(registry_url, root, root_deps, pnpmfile_src)
        .await
}

/// Same as [`install_with_pnpmfile`] but routes install events through the
/// given reporter, so a recording reporter can assert on the `pnpm:hook`
/// log channel.
async fn install_with_pnpmfile_reporter<Reporter: self::Reporter + 'static>(
    registry_url: &str,
    root: &std::path::Path,
    root_deps: &[(&str, &str)],
    pnpmfile_src: &str,
) -> Result<(), InstallError> {
    let modules_dir = root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let manifest_path = root.join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path).unwrap();
    for (name, spec) in root_deps {
        manifest.add_dependency(name, spec, DependencyGroup::Prod).unwrap();
    }
    manifest.save().unwrap();

    std::fs::write(root.join(".pnpmfile.cjs"), pnpmfile_src).unwrap();

    let mut config = Config::new();
    config.store_dir = root.join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir;
    config.registry = registry_url.to_string();
    let config = config.leak();

    let http_client = Default::default();
    Install {
        lockfile_policy: crate::InstallLockfilePolicy {
            frozen: false,
            prefer_frozen: None,
            ignore_manifest_check: false,
            trust: false,
            update_checksums: false,
            excludes: PolicyExcludes::Persist,
            disable_optimistic_repeat: false,
            manifest_freshness: crate::ManifestFreshness::Mtime,
        },
        execution: crate::InstallExecution {
            skip_runtimes: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            node_linker: pnpm_config::NodeLinker::default(),
            lockfile_only: false,
            dry_run: false,
        },
        resolution: crate::ResolutionInputs {
            update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
        },
        context: crate::InstallInvocation {
            http_client: &http_client,
            config,
            manifest: &manifest,
            emit_initial_manifest: true,
            lockfile: MaybeLazyLockfile::Loaded(None),
            lockfile_path: None,
        },
        fetching: crate::InstallFetching {
            tarball_mem_cache: Default::default(),
            http_client_arc: std::sync::Arc::new(Default::default()),
            resolved_packages: &Default::default(),
        },
        projects: crate::InstallProjects {
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ],
            supported_architectures: None,
            catalogs_override: None,
            pnpmfile_hook_override: None,
            policy_excludes_dir: None,
            workspace_projects_override: None,
        },
    }
    .run::<Reporter>()
    .await
}

/// Same as [`install_with_pnpmfile`] but for a workspace whose member
/// carries the dependencies, so a hook rewriting a member's own
/// specifier is exercised rather than only the root's.
async fn install_workspace_member_with_pnpmfile(
    registry_url: &str,
    root: &std::path::Path,
    member_deps: &[(&str, &str)],
    pnpmfile_src: &str,
) -> Result<(), InstallError> {
    let member_dir = root.join("packages/member");
    std::fs::create_dir_all(&member_dir).unwrap();
    std::fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n").unwrap();

    let dependencies: serde_json::Map<_, _> = member_deps
        .iter()
        .map(|(name, spec)| ((*name).to_string(), serde_json::Value::from(*spec)))
        .collect();
    std::fs::write(
        member_dir.join("package.json"),
        serde_json::json!({
            "name": "member",
            "version": "1.0.0",
            "dependencies": dependencies,
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        root.join("package.json"),
        serde_json::json!({ "name": "workspace-root", "version": "1.0.0" }).to_string(),
    )
    .unwrap();
    let manifest = PackageManifest::from_path(root.join("package.json")).unwrap();

    std::fs::write(root.join(".pnpmfile.cjs"), pnpmfile_src).unwrap();

    let modules_dir = root.join("node_modules");
    let mut config = Config::new();
    config.workspace_dir = Some(root.to_path_buf());
    config.store_dir = root.join("pacquet-store").into();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    config.modules_dir = modules_dir;
    config.registry = registry_url.to_string();
    let config = config.leak();

    let http_client = Default::default();
    Install {
        lockfile_policy: crate::InstallLockfilePolicy {
            frozen: false,
            prefer_frozen: None,
            ignore_manifest_check: false,
            trust: false,
            update_checksums: false,
            excludes: PolicyExcludes::Persist,
            disable_optimistic_repeat: false,
            manifest_freshness: crate::ManifestFreshness::Mtime,
        },
        execution: crate::InstallExecution {
            skip_runtimes: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            node_linker: pnpm_config::NodeLinker::default(),
            lockfile_only: false,
            dry_run: false,
        },
        resolution: crate::ResolutionInputs {
            update_seed_policy: crate::UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
        },
        context: crate::InstallInvocation {
            http_client: &http_client,
            config,
            manifest: &manifest,
            emit_initial_manifest: true,
            lockfile: MaybeLazyLockfile::Loaded(None),
            lockfile_path: None,
        },
        fetching: crate::InstallFetching {
            tarball_mem_cache: Default::default(),
            http_client_arc: std::sync::Arc::new(Default::default()),
            resolved_packages: &Default::default(),
        },
        projects: crate::InstallProjects {
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ],
            supported_architectures: None,
            catalogs_override: None,
            pnpmfile_hook_override: None,
            policy_excludes_dir: None,
            workspace_projects_override: None,
        },
    }
    .run::<SilentReporter>()
    .await
}

/// The first `pnpm:hook` event in `events`, or panic.
fn first_hook_log(events: &[LogEvent]) -> &HookLog {
    events
        .iter()
        .find_map(|event| match event {
            LogEvent::Hook(log) => Some(log),
            _ => None,
        })
        .expect("a pnpm:hook event must be emitted")
}

/// The `pnpm:global` messages one report emits, at `info` — the only
/// level these carry.
fn recorded_verified_file_integrity_report(verified: VerifiedFileIntegrity) -> Vec<String> {
    static MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());
    // The recorder is one shared static, so one caller uses it at a
    // time. `cargo nextest` gives each test its own process, but a
    // plain `cargo test` runs them as threads of one.
    static RECORDER: Mutex<()> = Mutex::new(());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            if let LogEvent::Global(log) = event {
                assert_eq!(log.level, LogLevel::Info);
                MESSAGES
                    .lock()
                    .unwrap()
                    .push(log.message.clone());
            }
        }
    }

    let _guard = RECORDER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    MESSAGES.lock().unwrap().clear();
    report_verified_file_integrity::<RecordingReporter>(verified);
    let messages = MESSAGES.lock().unwrap().clone();
    dbg!(messages)
}

fn assert_purge_diagnostic(error: &InstallError, path: &std::path::Path) {
    let rendered = error.to_string();
    assert!(rendered.contains(&path.display().to_string()), "got: {rendered}");
    assert!(rendered.contains("denied"), "source error must survive: {rendered}");
    assert_eq!(
        miette::Diagnostic::code(error).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR"),
    );

    let source = std::error::Error::source(error).expect("the io error stays in the source chain");
    let io_error =
        source.downcast_ref::<std::io::Error>().expect("the source is the original io::Error");
    assert_eq!(io_error.kind(), std::io::ErrorKind::PermissionDenied);
}
