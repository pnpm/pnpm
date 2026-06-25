use super::{create_deploy_install_config, split_local_payload};
use pacquet_config::{Config, NodeLinker};
use std::path::Path;

#[cfg(windows)]
use super::{is_ancestor_path, is_child_path, same_path, validate_deploy_target};

#[test]
fn split_local_payload_preserves_parentheses_in_path_before_peer_suffix() {
    assert_eq!(
        split_local_payload("../local(foo)/pkg(peer@1.0.0)"),
        ("../local(foo)/pkg", "(peer@1.0.0)"),
    );
}

#[test]
fn split_local_payload_preserves_parentheses_in_path_before_patch_suffix() {
    assert_eq!(
        split_local_payload("../local(foo)/pkg(patch_hash=abc)(peer@1.0.0)"),
        ("../local(foo)/pkg", "(patch_hash=abc)(peer@1.0.0)"),
    );
}

#[test]
fn hoisted_deploy_install_config_preserves_lockfile_setting() {
    let deploy_dir = Path::new("deploy");
    let mut base_config = Config::new();
    base_config.lockfile = true;

    let deploy_config = create_deploy_install_config(&base_config, deploy_dir, NodeLinker::Hoisted);
    assert!(deploy_config.lockfile);
    assert_eq!(deploy_config.node_linker, NodeLinker::Hoisted);

    base_config.lockfile = false;
    let deploy_config = create_deploy_install_config(&base_config, deploy_dir, NodeLinker::Hoisted);
    assert!(!deploy_config.lockfile);
}

#[cfg(windows)]
#[test]
fn windows_path_comparison_matches_case_variants() {
    assert!(same_path(Path::new("C:\\Workspace"), Path::new("c:\\workspace")));
    assert!(is_child_path(Path::new("c:\\workspace\\out"), Path::new("C:\\Workspace"),));
    assert!(is_ancestor_path(
        Path::new("c:\\workspace"),
        Path::new("C:\\Workspace\\packages\\app"),
    ));
}

#[cfg(windows)]
#[test]
fn windows_case_variant_workspace_root_is_rejected_as_deploy_target() {
    let err = validate_deploy_target(
        Path::new("c:\\workspace"),
        Path::new("C:\\Workspace"),
        Path::new("C:\\Workspace\\packages\\app"),
        Path::new("C:\\Workspace"),
        true,
    )
    .expect_err("case-variant workspace root must be rejected");
    assert!(err.to_string().contains("target is the workspace root"));
}
