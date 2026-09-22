use super::super::build_workspace_state;
use pnpm_config::Config;
use pnpm_modules_yaml::{
    Clock,
    IncludedDependencies,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::ConfigDependency;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{
        Duration,
        SystemTime,
    },
};
use tempfile::tempdir;

/// Stands in for the wall clock so the fallback branch records a value
/// the assertions can name. Far enough in the past that a real mtime
/// never collides with it.
const FAKE_NOW_MS: i64 = 1_700_000_000_000;

struct FrozenClock;
impl Clock for FrozenClock {
    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(FAKE_NOW_MS as u64)
    }
}

fn write_manifest(dir: &std::path::Path, name: &str, version: &str) -> PackageManifest {
    let manifest_path = dir.join("package.json");
    std::fs::write(&manifest_path, format!(r#"{{"name":"{name}","version":"{version}"}}"#))
        .unwrap();
    PackageManifest::from_path(manifest_path).unwrap()
}

fn config_for(workspace_root: &std::path::Path) -> Config {
    let mut config = Config::new();
    config.virtual_store_dir = workspace_root.join("node_modules/.pnpm");
    config
}

/// With nothing on disk to take an mtime from, the timestamp falls
/// back to the clock.
#[test]
fn empty_project_list_produces_empty_projects_map() {
    let dir = tempdir().unwrap();
    let config = config_for(dir.path());
    let state = build_workspace_state::<FrozenClock>(
        dir.path(),
        &config,
        pnpm_config::NodeLinker::default(),
        IncludedDependencies::default(),
        None,
        &BTreeMap::default(),
        &[],
        false,
        None,
    );
    assert!(state.projects.is_empty());
    assert_eq!(state.last_validated_timestamp, FAKE_NOW_MS);
}

/// The recorded timestamp has to share a clock with the files the
/// freshness check later compares it against.
#[test]
fn prefers_the_manifest_mtime_over_the_clock() {
    let dir = tempdir().unwrap();
    let manifest = write_manifest(dir.path(), "root", "1.0.0");
    let config = config_for(dir.path());
    let state = build_workspace_state::<FrozenClock>(
        dir.path(),
        &config,
        pnpm_config::NodeLinker::default(),
        IncludedDependencies::default(),
        None,
        &BTreeMap::default(),
        &[(dir.path().to_path_buf(), &manifest)],
        false,
        None,
    );
    assert_eq!(state.last_validated_timestamp, pnpm_testing_utils::fs::mtime_ms(manifest.path()));
}

/// A manifest mtime truncated to milliseconds reads as modified
/// against itself on a sub-millisecond filesystem, so the recorded
/// baseline is raised to the filesystem clock's `now`, which the
/// caller reads off a probe file.
#[test]
fn raises_the_manifest_mtime_baseline_to_the_filesystem_now() {
    let dir = tempdir().unwrap();
    let manifest = write_manifest(dir.path(), "root", "1.0.0");
    pnpm_testing_utils::fs::set_mtime(
        manifest.path(),
        SystemTime::UNIX_EPOCH + Duration::new(1_600_000_000, 500_000),
    );
    let config = config_for(dir.path());
    let build = |filesystem_now_ms| {
        build_workspace_state::<FrozenClock>(
            dir.path(),
            &config,
            pnpm_config::NodeLinker::default(),
            IncludedDependencies::default(),
            None,
            &BTreeMap::default(),
            &[(dir.path().to_path_buf(), &manifest)],
            false,
            filesystem_now_ms,
        )
    };

    let now_ms = 1_600_000_001_000;
    assert_eq!(build(Some(now_ms)).last_validated_timestamp, now_ms);
    // A manifest dated ahead of the filesystem clock keeps its own
    // mtime as the baseline.
    let manifest_ms = pnpm_testing_utils::fs::mtime_ms(manifest.path());
    assert_eq!(build(Some(manifest_ms - 1)).last_validated_timestamp, manifest_ms);
}

/// Every project in the list lands in `state.projects` keyed by its
/// `root_dir`. Regression catch for the bug where a workspace fresh
/// install (no `pnpm-lock.yaml` on disk) recorded only the root
/// importer.
#[test]
fn records_every_workspace_project_keyed_by_root_dir() {
    let dir = tempdir().unwrap();
    let packages = ["a", "b", "c", "d"];
    let manifests: Vec<(PathBuf, PackageManifest)> = packages
        .iter()
        .map(|name| {
            let project_dir = dir.path().join("packages").join(name);
            std::fs::create_dir_all(&project_dir).unwrap();
            let manifest = write_manifest(&project_dir, name, "1.0.0");
            (project_dir, manifest)
        })
        .collect();
    let project_manifests: Vec<(PathBuf, &PackageManifest)> = manifests
        .iter()
        .map(|(p, m)| (p.clone(), m))
        .collect();

    let config = config_for(dir.path());
    let state = build_workspace_state::<FrozenClock>(
        dir.path(),
        &config,
        pnpm_config::NodeLinker::default(),
        IncludedDependencies::default(),
        None,
        &BTreeMap::default(),
        &project_manifests,
        false,
        None,
    );

    assert_eq!(state.projects.len(), packages.len());
    for (project_dir, _) in &manifests {
        let key = project_dir.to_string_lossy().into_owned();
        let entry = state.projects
            .get(&key)
            .unwrap_or_else(|| panic!("project entry for {key:?} should exist"));
        assert_eq!(entry.version.as_deref(), Some("1.0.0"));
        assert!(packages.contains(&entry.name.as_deref().unwrap_or_default(),));
    }
}

/// pnpm's `createWorkspaceState` records `configDependencies`
/// verbatim. When pacquet is the install engine for a project that
/// declares one (the `@pnpm/pacquet` configDependency itself), the
/// written state must carry the same map — otherwise pnpm's
/// `checkDepsStatus` reads a missing value, treats the install as
/// stale, and reinstalls on every `pnpm run` / `pnpm node`.
#[test]
fn records_config_dependencies_from_config() {
    let dir = tempdir().unwrap();
    let mut config = config_for(dir.path());
    config.config_dependencies = Some(BTreeMap::from([(
        "@pnpm/pacquet".to_string(),
        ConfigDependency::VersionWithIntegrity("0.2.2-14".to_string()),
    )]));
    let state = build_workspace_state::<FrozenClock>(
        dir.path(),
        &config,
        pnpm_config::NodeLinker::default(),
        IncludedDependencies::default(),
        None,
        &BTreeMap::default(),
        &[],
        false,
        None,
    );
    assert_eq!(state.config_dependencies, config.config_dependencies);
}
