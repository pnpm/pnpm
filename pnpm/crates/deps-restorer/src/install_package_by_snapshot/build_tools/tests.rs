use super::{BuildToolError, check_installed_identity};
use pnpm_lockfile::{BuildToolArtifact, LockfileResolution, PackageKey, build_tool_artifact};
use std::fs;

fn yarn_artifact() -> (PackageKey, LockfileResolution) {
    let key: PackageKey = "yarn@tool:4.9.2".parse().expect("parse key");
    let resolution = serde_saphyr::from_str(
        "{integrity: sha512-mQ==, tarball: https://registry.npmjs.org/@yarnpkg/cli-dist/-/cli-dist-4.9.2.tgz}",
    )
    .expect("parse resolution");
    (key, resolution)
}

#[test]
fn the_installed_tool_is_the_package_the_record_names() {
    let (key, resolution) = yarn_artifact();
    let artifact = build_tool_artifact(&key, &resolution).expect("a build tool");
    assert!(matches!(artifact, BuildToolArtifact::Registry { package: "@yarnpkg/cli-dist", .. }));
    let dir = tempfile::tempdir().unwrap();

    fs::write(dir.path().join("package.json"), r#"{"name":"@yarnpkg/cli-dist","version":"4.9.2"}"#)
        .unwrap();
    assert!(check_installed_identity(&key, artifact, dir.path()).is_ok());

    fs::write(dir.path().join("package.json"), r#"{"name":"not-yarn","version":"4.9.2"}"#).unwrap();
    let error = check_installed_identity(&key, artifact, dir.path()).expect_err("another package");
    assert!(matches!(error, BuildToolError::NotTheRecordedTool { .. }), "{error:?}");

    fs::write(dir.path().join("package.json"), r#"{"name":"@yarnpkg/cli-dist","version":"4.0.0"}"#)
        .unwrap();
    assert!(check_installed_identity(&key, artifact, dir.path()).is_err());
}
