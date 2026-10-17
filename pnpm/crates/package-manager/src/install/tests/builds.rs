use super::{
    super::{Install, InstallError, ProjectMutation, project_lifecycle_graph},
    InstallDirs,
};
use crate::{PolicyExcludes, ResolvedPackages};
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use pnpm_testing_utils::registry::TestRegistry;
use std::{fs, sync::Arc};
use tempfile::tempdir;
use text_block_macros::text_block;

#[test]
fn lifecycle_graph_normalizes_paths_and_recovers_from_incomplete_explicit_graph() {
    let temp = tempdir().unwrap();
    let workspace_root = temp.path().join("workspace");
    let dependency_dir = workspace_root.join("packages/holding/../dependency");
    let dependent_dir = workspace_root.join("packages/dependent");
    let dependency_manifest = PackageManifest::from_value(
        dependency_dir.join("package.json"),
        serde_json::json!({ "scripts": { "prepare": "node prepare.js" } }),
    );
    let dependent_manifest = PackageManifest::from_value(
        dependent_dir.join("package.json"),
        serde_json::json!({ "scripts": { "prepare": "node prepare.js" } }),
    );
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  packages/dependent:"
        "    dependencies:"
        "      dependency:"
        "        specifier: workspace:*"
        "        version: link:../dependency"
    })
    .unwrap();
    let incomplete_dependencies = indexmap::IndexMap::from([(dependent_dir.clone(), Vec::new())]);

    let Err(missing_order_error) = project_lifecycle_graph(
        &[
            (dependent_dir.clone(), &dependent_manifest),
            (dependency_dir.clone(), &dependency_manifest),
        ],
        Some(&incomplete_dependencies),
        &workspace_root,
        None,
    ) else {
        panic!("incomplete graph without a lockfile must fail");
    };
    assert!(matches!(missing_order_error, InstallError::ProjectLifecycleOrder { .. }));

    let graph = project_lifecycle_graph(
        &[
            (dependent_dir.clone(), &dependent_manifest),
            (dependency_dir.clone(), &dependency_manifest),
        ],
        Some(&incomplete_dependencies),
        &workspace_root,
        Some(&lockfile),
    )
    .unwrap();
    let dependency_dir = pnpm_fs::lexical_normalize(&dependency_dir);
    assert_eq!(graph.dependencies[&dependent_dir], vec![dependency_dir]);
}
/// `build_modules_manifest` serializes the install-time
/// [`SkippedSnapshots`] into `.modules.yaml.skipped` as a list of
/// depPath strings: each entry is the snapshot's [`PackageKey`]
/// `Display` form (`name@version(peers)`), and ordering is handled by
/// `write_modules_manifest`'s sort-on-write.
///
/// An empty set produces an empty list — covers the fresh-install
/// case while pinning that the field is always present on the
/// manifest.
///
/// [`SkippedSnapshots`]: super::super::super::super::SkippedSnapshots
/// [`PackageKey`]: pnpm_lockfile::PackageKey
#[test]
fn build_modules_manifest_serializes_skipped_set() {
    use crate::SkippedSnapshots;
    use pnpm_lockfile::PackageKey;
    use pnpm_modules_yaml::IncludedDependencies;
    use std::collections::HashSet;

    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.install_state_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();

    let key1: PackageKey = "darwin-only-pkg@1.0.0".parse().unwrap();
    let key2: PackageKey = "@scope/linux-only@2.3.4".parse().unwrap();
    let mut set = HashSet::new();
    set.insert(key1.clone());
    set.insert(key2.clone());
    let skipped = SkippedSnapshots::from_set(set);

    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: true,
    };
    let manifest = super::super::build_modules_manifest(
        config,
        pnpm_config::NodeLinker::default(),
        included,
        Default::default(),
        Default::default(),
        Default::default(),
        &skipped,
        &[],
        Vec::new(),
        "Thu, 01 Jan 1970 00:00:00 GMT".to_string(),
    );

    // Compare as sets — `build_modules_manifest` does not sort.
    // Sort-on-write happens later inside `write_modules_manifest`;
    // the read-after-write order is covered by the integration
    // test on the full install path.
    let actual: HashSet<String> = manifest.skipped
        .iter()
        .cloned()
        .collect();
    let expected: HashSet<String> = [key1.to_string(), key2.to_string()].into_iter().collect();
    assert_eq!(actual, expected);
}
/// Empty `SkippedSnapshots` produces an empty `Modules.skipped`. The
/// common case — most installs have no platform-mismatched optional
/// deps — must keep the field present-but-empty so the on-disk
/// shape stays uniform.
#[test]
fn build_modules_manifest_skipped_is_empty_on_empty_set() {
    use crate::SkippedSnapshots;
    use pnpm_modules_yaml::IncludedDependencies;

    let dir = tempdir().unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.install_state_dir = dir.path().join("node_modules/.pacquet");
    let config = config.leak();

    let manifest = super::super::build_modules_manifest(
        config,
        pnpm_config::NodeLinker::default(),
        IncludedDependencies::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        &SkippedSnapshots::new(),
        &[],
        Vec::new(),
        "Thu, 01 Jan 1970 00:00:00 GMT".to_string(),
    );
    assert!(manifest.skipped.is_empty());
    // Empty `hoisted_locations` is dropped to `None` so an
    // isolated install doesn't write a `hoistedLocations: {}` key
    // (which would falsely look like a stale hoisted-mode write).
    assert!(manifest.hoisted_locations.is_none());
}
#[tokio::test]
async fn fresh_install_uses_final_peer_suffix_for_transitive_pending_peer() {
    let mock_instance = TestRegistry::start();

    let dirs = InstallDirs::new();

    let manifest_path = dirs.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();
    manifest.add_dependency("@pnpm.e2e/final-peer-a", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.add_dependency("@pnpm.e2e/final-peer-c", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let mut config = Config::new();
    config.store_dir = dirs.store_dir.clone().into();
    config.modules_dir = dirs.modules_dir.clone();
    config.install_state_dir = dirs.virtual_store_dir.clone();
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
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ],
            supported_architectures: None,
            catalogs_override: None,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
            dedicated: None,
        },
    }
    .run::<SilentReporter>()
    .await
    .expect("install should succeed");

    let content = std::fs::read_to_string(dirs.path().join(Lockfile::FILE_NAME))
        .expect("read pnpm-lock.yaml");
    let expected = "@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0(@pnpm.e2e/final-peer-c@1.0.0)))";
    let provisional =
        "@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0))";

    assert!(
        content.contains(expected),
        "transitive peer must use the provider's final peer suffix; lockfile:\n{content}",
    );
    assert!(
        !content.contains(provisional),
        "lockfile must not keep the provider's provisional peer suffix; lockfile:\n{content}",
    );

    drop((dirs.dir, mock_instance));
}
/// `unapproved_recorded_ignored_builds` surfaces a malformed
/// `allowBuilds` spec as `Err` (here `ERR_PNPM_INVALID_VERSION_UNION`)
/// instead of swallowing it into `None`, so the strict up-to-date fast
/// paths fall through to the full install — which reports the real error
/// — rather than short-circuiting to success and hiding it.
#[test]
fn unapproved_recorded_ignored_builds_surfaces_invalid_allow_builds() {
    let modules = pnpm_modules_yaml::ModulesLayout {
        ignored_builds: Some([pnpm_modules_yaml::DepPath::from("pkg@1.0.0".to_string())].into()),
        ..Default::default()
    };
    let mut config = Config::new();
    config.allow_builds.insert("foo@not-a-version".to_string(), true);
    let config = config.leak();

    assert!(super::super::unapproved_recorded_ignored_builds(&modules, config).is_err());
}

async fn install_in_memory_project(dirs: &InstallDirs, manifest: &PackageManifest) {
    let config = Config {
        store_dir: dirs.store_dir.clone().into(),
        cache_dir: dirs.path().join("cache"),
        modules_dir: dirs.modules_dir.clone(),
        install_state_dir: dirs.virtual_store_dir.clone(),
        extra_bin_paths: vec![dirs.project_root.join("bin")],
        offline: true,
        lockfile: false,
        workspace_search_skipped: true,
        ..Config::new()
    }
    .leak();
    let resolved_packages = ResolvedPackages::default();
    let http_client: Arc<pnpm_network::ThrottledClient> = Arc::default();
    Install::new(
        Arc::default(),
        &resolved_packages,
        (&http_client, Arc::clone(&http_client)),
        config,
        manifest,
        MaybeLazyLockfile::Loaded(None),
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    )
    .run::<SilentReporter>()
    .await
    .expect("install in-memory project");
}

#[tokio::test]
async fn native_preparation_preserves_in_memory_non_native_scripts() {
    for value in [
        serde_json::json!({ "gypfile": false, "scripts": { "postinstall": "node marker.cjs" } }),
        serde_json::json!({ "scripts": { "install": "node marker.cjs" } }),
        serde_json::json!({ "scripts": { "preinstall": "node marker.cjs" } }),
    ] {
        let dirs = InstallDirs::new();
        fs::create_dir_all(&dirs.project_root).unwrap();
        fs::write(dirs.project_root.join("binding.gyp"), "{}").unwrap();
        fs::write(
            dirs.project_root.join("marker.cjs"),
            "require('fs').writeFileSync('explicit-marker', 'ran')",
        )
        .unwrap();
        let manifest = PackageManifest::from_value(dirs.project_root.join("package.json"), value);
        install_in_memory_project(&dirs, &manifest).await;
        assert!(!manifest.path().exists(), "published a non-native API manifest");
        assert!(
            !dirs.project_root.join("explicit-marker").exists(),
            "activated an explicit script",
        );
    }
}

#[tokio::test]
async fn native_preparation_preserves_existing_alternate_manifests() {
    for (name, contents) in [
        (
            "package.json5",
            "{version: '2.0.0', gypfile: false, scripts: {install: 'node marker.cjs'}}\n",
        ),
        ("package.yaml", "version: 2.0.0\ngypfile: false\nscripts:\n  install: node marker.cjs\n"),
    ] {
        let dirs = InstallDirs::new();
        let bin = dirs.project_root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(dirs.project_root.join(name), contents).unwrap();
        fs::write(dirs.project_root.join("binding.gyp"), "{}").unwrap();
        fs::write(
            dirs.project_root.join("marker.cjs"),
            "require('fs').writeFileSync('alternate-marker', process.env.npm_package_version)",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let compiler = bin.join("node-gyp");
            fs::write(&compiler, "#!/bin/sh\nexit 17\n").unwrap();
            fs::set_permissions(&compiler, fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(windows)]
        fs::write(bin.join("node-gyp.cmd"), "@exit /b 17\r\n").unwrap();
        let manifest = PackageManifest::from_value(
            dirs.project_root.join("package.json"),
            serde_json::json!({}),
        );
        install_in_memory_project(&dirs, &manifest).await;
        assert!(!manifest.path().exists(), "shadowed {name}");
        assert_eq!(fs::read_to_string(dirs.project_root.join(name)).unwrap(), contents);
        assert_eq!(
            fs::read_to_string(dirs.project_root.join("alternate-marker")).unwrap(),
            "2.0.0",
        );
    }
}
