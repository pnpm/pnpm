use super::{
    super::{TINY_BACKEND, assert_failure_contains, running_platform, serve, wheel},
    approve, declare_platforms, project, repository,
};
use crate::_utils::pacquet_in;

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
