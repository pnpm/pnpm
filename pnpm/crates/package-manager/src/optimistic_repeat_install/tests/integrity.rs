use super::{
    super::{Decision, settings::current_settings},
    check, isolated_included, write_empty_lockfile, write_state,
};
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::backdate_existing_files;
use pnpm_workspace_state::ProjectEntry;
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

/// `explicit_settings` stands in for pnpm's raw (default-`undefined`)
/// config value, which is what gates whether the policy is recorded.
#[test]
fn returns_skipped_when_trust_policy_is_newly_configured() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let mut stale_config = Config::new();
    stale_config.modules_dir = workspace_root.join("node_modules");
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    assert_eq!(stale_settings.trust_policy, None, "an unconfigured policy is not recorded");
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.trust_policy = pnpm_config::TrustPolicy::NoDowngrade;
    config
        .explicit_settings
        .insert("trustPolicy".to_string(), serde_json::Value::String("no-downgrade".to_string()));
    let config = config.leak();

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}
