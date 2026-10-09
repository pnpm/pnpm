//! `enableModulesDir: false`: no `node_modules`, but a populated store.

use super::super::{Install, ProjectMutation};
use crate::PolicyExcludes;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, LockfileResolution, MaybeLazyLockfile};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::{StoreIndex, store_index_key};
use pnpm_testing_utils::registry::TestRegistry;
use std::path::Path;
use tempfile::tempdir;

const INCOMPATIBLE: &str = "@pnpm.e2e/not-compatible-with-any-os";

struct Project {
    dir: tempfile::TempDir,
    config: &'static Config,
    manifest: PackageManifest,
}

fn project(registry: &TestRegistry) -> Project {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");
    let mut manifest = PackageManifest::create_if_needed(dir.path().join("package.json")).unwrap();
    manifest.add_dependency("@pnpm.e2e/pkg-with-1-dep", "100.0.0", DependencyGroup::Prod).unwrap();
    manifest.add_dependency(INCOMPATIBLE, "1.0.0", DependencyGroup::Optional).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.install_state_dir = modules_dir.join(".pacquet");
    config.modules_dir = modules_dir;
    config.registry = registry.url().to_string();
    config.enable_modules_dir = false;
    Project { dir, config: config.leak(), manifest }
}

/// `lockfile_only` is the explicit `--lockfile-only` and `frozen` the
/// explicit `--frozen-lockfile`; the config's `enable_modules_dir = false`
/// is set either way.
async fn install(project: &Project, lockfile_only: bool, frozen: bool) {
    let lockfile = frozen.then(|| read_lockfile(project.dir.path()));
    Install {
        lockfile_policy: crate::InstallLockfilePolicy {
            frozen,
            prefer_frozen: Some(frozen),
            ignore_manifest_check: false,
            trust: false,
            update_checksums: false,
            excludes: PolicyExcludes::Persist,
            disable_optimistic_repeat: true,
            manifest_freshness: crate::ManifestFreshness::Mtime,
        },
        execution: crate::InstallExecution {
            skip_runtimes: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            node_linker: pnpm_config::NodeLinker::default(),
            lockfile_only,
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
            config: project.config,
            manifest: &project.manifest,
            emit_initial_manifest: true,
            lockfile: MaybeLazyLockfile::Loaded(lockfile.as_ref()),
            lockfile_path: None,
        },
        fetching: crate::InstallFetching {
            tarball_mem_cache: Default::default(),
            http_client_arc: std::sync::Arc::new(Default::default()),
            resolved_packages: &Default::default(),
        },
        projects: crate::InstallProjects {
            dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
            supported_architectures: None,
            catalogs_override: None,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
            dedicated: None,
        },
    }
    .run::<SilentReporter>()
    .await
    .expect("install without a modules dir should succeed");
}

fn read_lockfile(dir: &Path) -> Lockfile {
    let content = std::fs::read_to_string(dir.join(Lockfile::FILE_NAME)).expect("read lockfile");
    serde_saphyr::from_str(&content).expect("parse lockfile")
}

/// The lockfile's registry packages, split into the ones the host can
/// install and the one it cannot, each as its store-index key.
fn store_keys(lockfile: &Lockfile) -> (Vec<String>, String) {
    let mut installable = Vec::new();
    let mut incompatible = None;
    for (key, metadata) in lockfile.packages.as_ref().expect("resolved packages") {
        let LockfileResolution::Registry(registry) = &metadata.resolution else {
            panic!("fixture packages resolve to the registry");
        };
        let store_key = store_index_key(&registry.integrity.to_string(), &key.pkg_id());
        if key.pkg_id().starts_with(INCOMPATIBLE) {
            incompatible = Some(store_key);
        } else {
            installable.push(store_key);
        }
    }
    assert!(installable.len() >= 2, "the fixture pulls in a dependency of its own");
    (installable, incompatible.expect("the optional dependency is resolved"))
}

fn keys_in_store(config: &Config, keys: &[String]) -> Vec<String> {
    let Some(index) = StoreIndex::shared_readonly_in(&config.store_dir) else {
        return Vec::new();
    };
    let found = index
        .lock()
        .expect("lock index")
        .contains_many(keys)
        .expect("query index");
    let mut found: Vec<String> = found.into_iter().collect();
    found.sort();
    found
}

#[tokio::test]
async fn fresh_install_without_modules_dir_fetches_the_packages_into_the_store() {
    let registry = TestRegistry::start();
    let project = project(&registry);

    install(&project, false, false).await;

    assert!(!project.config.modules_dir.exists(), "no node_modules is written");
    let lockfile = read_lockfile(project.dir.path());
    let (mut installable, incompatible) = store_keys(&lockfile);
    installable.sort();
    assert_eq!(keys_in_store(project.config, &installable), installable);
    assert!(
        keys_in_store(project.config, &[incompatible]).is_empty(),
        "another platform's package is not fetched"
    );
}

#[tokio::test]
async fn frozen_install_without_modules_dir_fetches_the_packages_into_the_store() {
    let registry = TestRegistry::start();
    let project = project(&registry);

    // `--lockfile-only` writes the lockfile and still fetches nothing.
    install(&project, true, false).await;
    let lockfile = read_lockfile(project.dir.path());
    let (mut installable, incompatible) = store_keys(&lockfile);
    installable.sort();
    assert!(
        keys_in_store(project.config, &installable).is_empty(),
        "a lockfile-only run fetches nothing"
    );

    install(&project, false, true).await;

    assert!(!project.config.modules_dir.exists(), "no node_modules is written");
    assert_eq!(keys_in_store(project.config, &installable), installable);
    assert!(
        keys_in_store(project.config, &[incompatible]).is_empty(),
        "another platform's package is not fetched"
    );
}
