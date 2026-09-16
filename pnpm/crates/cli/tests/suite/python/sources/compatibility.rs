use super::{
    super::{TINY_BACKEND, assert_failure_contains, python, running_platform, serve, wheel},
    approve, declare_platforms, project, repository,
};
use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[tokio::test]
async fn git_wheels_must_be_compatible_with_every_declared_target() {
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let backend = TINY_BACKEND.replace("py3-none-any", running_platform().1);
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("1.0", wheel("tinybackend", "1.0", "", &[("tinybuild.py", &backend)]))],
    )
    .await;
    project(root.path(), &server.url(), &[&format!("fork @ git+{url}@{commit}")]);
    approve(root.path(), "fork");
    declare_platforms(root.path());
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "Python wheel is incompatible with this interpreter",
    );
    assert!(!root.path().join("pylock.toml").exists());
}

#[tokio::test]
async fn equivalent_direct_wheel_hashes_merge_across_declared_targets() {
    for omit_hash in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let archive = wheel("alpha", "1.0", "", &[]);
        let digest = format!("{:x}", Sha256::digest(&archive));
        let url = format!("{}/alpha-1.0-py3-none-any.whl", server.url());
        let alternative =
            if omit_hash { url.clone() } else { format!("{url}#sha256={}", digest.to_uppercase()) };
        let artifact = server
            .mock("GET", "/alpha-1.0-py3-none-any.whl")
            .with_body(archive)
            .expect(1)
            .create_async()
            .await;
        project(
            root.path(),
            &server.url(),
            &[
                &format!("alpha @ {url}#sha256={digest} ; sys_platform == 'win32'"),
                &format!("alpha @ {alternative} ; sys_platform != 'win32'"),
            ],
        );
        declare_platforms(root.path());
        assert_single_wheel_replays(root.path());
        artifact.assert_async().await;
    }
}

fn assert_single_wheel_replays(root: &Path) {
    pacquet_in(root)
        .arg("install")
        .assert()
        .success();
    let contents = fs::read_to_string(root.join("pylock.toml")).unwrap();
    let lock: pnpm_python_resolver::Lockfile = toml::from_str(&contents).unwrap();
    assert_eq!(
        lock.packages
            .iter()
            .filter(|package| package.name.as_ref() == "alpha")
            .count(),
        1,
    );
    pacquet_in(root)
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    python(root)
        .args(["-c", "import alpha"])
        .assert()
        .success();
}
