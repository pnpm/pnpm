use super::{ConfigOverlay, build_overlay, install_options, resolve_config};

#[test]
fn resolve_config_reloads_changed_workspace_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "ignoreScripts: true\n")
        .expect("write workspace yaml");
    let first = resolve_config(dir.path(), &ConfigOverlay::default()).expect("first config");
    assert!(first.ignore_scripts);

    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "ignoreScripts: false\n")
        .expect("rewrite workspace yaml");
    let second = resolve_config(dir.path(), &ConfigOverlay::default()).expect("second config");
    assert!(!second.ignore_scripts);
}

#[test]
fn build_overlay_parses_link_workspace_packages() {
    use pnpm_config::LinkWorkspacePackages;

    let mut options = install_options();
    options.link_workspace_packages = Some(serde_json::json!("deep"));
    assert_eq!(
        build_overlay(&options, false).expect("overlay").link_workspace_packages,
        Some(LinkWorkspacePackages::Deep),
    );

    options.link_workspace_packages = Some(serde_json::json!(true));
    assert_eq!(
        build_overlay(&options, false).expect("overlay").link_workspace_packages,
        Some(LinkWorkspacePackages::DirectOnly),
    );

    options.link_workspace_packages = Some(serde_json::json!(false));
    assert_eq!(
        build_overlay(&options, false).expect("overlay").link_workspace_packages,
        Some(LinkWorkspacePackages::Off),
    );

    // Anything other than a boolean or "deep" is rejected.
    options.link_workspace_packages = Some(serde_json::json!("shallow"));
    assert!(build_overlay(&options, false).is_err());
}
