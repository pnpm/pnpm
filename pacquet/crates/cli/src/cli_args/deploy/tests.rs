use super::{
    ConvertCtx, convert_package_metadata, create_deploy_install_config, split_local_payload,
    validate_lockfile_local_path,
};
use pacquet_config::{Config, NodeLinker};
use pacquet_lockfile::{LockfileResolution, PackageMetadata, TarballResolution};
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

#[test]
fn lockfile_local_path_rejects_workspace_escape() {
    let workspace = Path::new("workspace");
    assert!(
        validate_lockfile_local_path(&workspace.join("packages/app"), workspace).is_ok(),
        "workspace children should be valid lockfile local paths",
    );

    let err = validate_lockfile_local_path(&workspace.join("../outside"), workspace)
        .expect_err("parent traversal should be rejected");
    assert!(err.to_string().contains("outside workspace"), "unexpected error: {err}");
}

#[test]
fn convert_package_metadata_rebases_file_tarball_resolution_to_deploy_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lockfile_dir = tmp.path().join("workspace");
    let deploy_dir = lockfile_dir.join("deploy");
    let deployed_project_root = lockfile_dir.join("packages/app");
    let all_projects = Vec::new();
    let metadata = PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "file:vendor/pkg.tgz".to_string(),
            integrity: Some(
                "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                    .parse()
                    .expect("parse integrity"),
            ),
            git_hosted: None,
            path: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    };
    let ctx = ConvertCtx {
        all_projects: &all_projects,
        deploy_dir: &deploy_dir,
        lockfile_dir: &lockfile_dir,
        deployed_project_root: &deployed_project_root,
    };

    let converted = convert_package_metadata(&metadata, &ctx).expect("convert metadata");

    match converted.resolution {
        LockfileResolution::Tarball(resolution) => {
            assert_eq!(resolution.tarball, "file:../vendor/pkg.tgz");
        }
        other => panic!("expected tarball resolution, got {other:?}"),
    }
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
