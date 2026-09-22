use super::{
    super::{
        assert_failure_contains,
        python,
        python_project,
        serve,
        serve_backends,
        wheel,
    },
    approve,
    git,
    project,
    repository,
};
use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use std::fs;

#[tokio::test]
async fn inactive_git_sources_in_frozen_lockfiles_are_rejected_before_building() {
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _index = serve(&mut server, "fork", &[("1.0", wheel("fork", "1.0", "", &[]))]).await;
    let direct = format!("git+{url}@{commit}");
    project(
        root.path(),
        &server.url(),
        &["fork", &format!("fork @ {direct} ; sys_platform == 'never'")],
    );
    approve(root.path(), "fork");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let path = root.path().join("pylock.toml");
    let mut lock: pnpm_python_resolver::Lockfile =
        toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let entry = lock.packages
        .iter_mut()
        .find(|package| package.name.as_ref() == "fork")
        .unwrap();
    let pnpm_python_resolver::Source::Git(mut vcs) =
        pnpm_python_resolver::Source::parse(&direct).unwrap()
    else {
        panic!("Git source expected")
    };
    vcs.commit_id = commit;
    entry.wheels.clear();
    entry.vcs = Some(vcs);
    fs::write(path, toml::to_string(&lock).unwrap()).unwrap();
    let trace = root.path().join("git-trace");
    assert_failure_contains(
        pacquet_in(root.path())
            .env("GIT_TRACE", &trace)
            .args(["install", "--frozen-lockfile"]),
        "inactive source",
    );
    let invocations = fs::read_to_string(trace).unwrap_or_default();
    assert!(!invocations.contains("built-in: git clone"), "{invocations}");
}

#[tokio::test]
async fn production_subsets_keep_sources_validated_in_the_complete_lock_graph() {
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _index = serve(&mut server, "fork", &[("1.0", wheel("fork", "1.0", "", &[]))]).await;
    let _dev = serve(
        &mut server,
        "devhelper",
        &[(
            "1.0",
            wheel("devhelper", "1.0", &format!("Requires-Dist: fork @ git+{url}@{commit}\n"), &[]),
        )],
    )
    .await;
    project(root.path(), &server.url(), &["fork"]);
    approve(root.path(), "fork");
    let manifest = root.path().join("pyproject.toml");
    let contents = fs::read_to_string(&manifest).unwrap();
    fs::write(manifest, format!("{contents}\n[dependency-groups]\ndev = ['devhelper']\n")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--prod"])
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--prod", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    let contents = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    let lock: pnpm_python_resolver::Lockfile = toml::from_str(&contents).unwrap();
    assert!(
        lock.packages
            .iter()
            .any(|package| package.name.as_ref() == "fork" && package.vcs.is_some()),
    );
    python(root.path()).args(["-c", "import fork, importlib.util; assert fork.MARKER == 'workspace fork'; assert importlib.util.find_spec('devhelper') is None"]).assert().success();
}

#[tokio::test]
async fn frozen_git_dependencies_activate_transitive_sources_from_verified_metadata() {
    let parent = tempfile::tempdir().unwrap();
    let child = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let (child_url, _) = repository(child.path());
    fs::remove_dir_all(child.path().join("src/fork")).unwrap();
    python_project(child.path(), "aardvark", "dependencies = []");
    let child_commit = commit_changes(child.path());
    let (parent_url, _) = repository(parent.path());
    python_project(
        parent.path(),
        "fork",
        &format!("dependencies = ['aardvark @ git+{child_url}@{child_commit}']"),
    );
    let parent_commit = commit_changes(parent.path());
    project(root.path(), &server.url(), &[&format!("fork @ git+{parent_url}@{parent_commit}")]);
    approve(root.path(), "fork");
    approve(root.path(), "aardvark");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    drop(parent);
    drop(child);
    pacquet_in(root.path())
        .env("GIT_ALLOW_PROTOCOL", "https")
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import fork, aardvark"])
        .assert()
        .success();
}

fn commit_changes(root: &std::path::Path) -> String {
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "dependencies",
        ],
    );
    git(root, &["rev-parse", "HEAD"])
}

#[tokio::test]
async fn frozen_git_sources_cannot_replace_an_explicit_commit_pin() {
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[&format!("fork @ git+{url}@{commit}")]);
    approve(root.path(), "fork");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    fs::write(repo.path().join("src/fork/__init__.py"), "raise RuntimeError('replaced commit')\n")
        .unwrap();
    let replacement = commit_changes(repo.path());
    let path = root.path().join("pylock.toml");
    let mut lock: pnpm_python_resolver::Lockfile =
        toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    lock.packages
        .iter_mut()
        .find(|package| package.name.as_ref() == "fork")
        .unwrap()
        .vcs
        .as_mut()
        .unwrap()
        .commit_id = replacement;
    fs::write(path, toml::to_string(&lock).unwrap()).unwrap();
    let trace = root.path().join("git-trace");
    assert_failure_contains(
        pacquet_in(root.path())
            .env("GIT_TRACE", &trace)
            .args(["install", "--frozen-lockfile"]),
        "does not satisfy the source",
    );
    let invocations = fs::read_to_string(trace).unwrap_or_default();
    assert!(!invocations.contains("built-in: git clone"), "{invocations}");
}
