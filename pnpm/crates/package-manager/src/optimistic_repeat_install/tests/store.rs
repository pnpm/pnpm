use super::{
    super::{Decision, settings::current_settings},
    check, isolated_included, setup_fresh_install, write_state,
};
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::backdate_existing_files;
use pnpm_workspace_state::ProjectEntry;
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

/// Drift in `enableGlobalVirtualStore` invalidates the cached state.
/// Toggling it moves the virtual store between `<storeDir>/links` and
/// each project's `node_modules/.pnpm`, so the previous install's
/// layout no longer matches a fresh resolution. The toggle is invisible
/// to the freshness check unless the key joins the comparison.
#[test]
fn returns_skipped_when_enable_global_virtual_store_drifts() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.enable_global_virtual_store = true;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.enable_global_virtual_store = false;
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
/// A pnpm-written state that records `enableGlobalVirtualStore: false`
/// (the value pnpm forces under CI) stays on the fast path for a pacquet
/// install with the store off, which omits the key. `false` and the
/// omitted `None` are the same "store off" state, so the coercion in
/// `enable_global_virtual_store_match` keeps the cross-package-manager
/// file from tripping a needless reinstall.
#[test]
fn returns_up_to_date_when_recorded_global_virtual_store_is_explicit_off() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");

    let mut settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    settings.enable_global_virtual_store = Some(false);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_existing_files(dir.path()), settings, projects);

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}
