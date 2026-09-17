use super::{project, python, python_project, serve, serve_backends, wheel};
use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use std::{fs, path::Path, process::Command};

/// A workspace project that declares nothing but its requirements.
fn requirements_project(root: &Path, name: &str, dependencies: &[&str]) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("pyproject.toml"),
        format!(
            "[project]\nname = '{name}'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
             dependencies = {dependencies:?}\n",
        ),
    )
    .unwrap();
}

/// Under `--fail-if-no-match`, pnpm prints its empty-selection notice to
/// stdout and exits 1.
fn assert_no_projects_matched(command: &mut Command) {
    let result = command.assert().failure();
    let stdout = String::from_utf8_lossy(&result.get_output().stdout).into_owned();
    assert!(stdout.contains("No projects matched the filters"), "stdout: {stdout}");
}

fn installed(root: &Path, project: &str) -> bool {
    root.join(project)
        .join("pylock.toml")
        .is_file()
}

/// The workspace here holds no npm project at all, so the selector has
/// nothing but the Python projects to name. `--fail-if-no-match` proves
/// that naming one counts as a match.
#[tokio::test]
async fn a_filter_selects_python_projects_by_the_distribution_they_declare() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let _beta = serve(&mut server, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    requirements_project(root.path(), "a", &["alpha"]);
    requirements_project(root.path(), "b", &["beta"]);

    pacquet_in(root.path())
        .args(["install", "--filter", "a", "--fail-if-no-match"])
        .assert()
        .success();

    assert!(installed(root.path(), "a"));
    assert!(!installed(root.path(), "b"), "b was not selected");
    assert!(!root.path().join("b/.venv").exists(), "b was not selected");
    python(&root.path().join("a"))
        .args(["-c", "import alpha"])
        .assert()
        .success();
}

#[tokio::test]
async fn a_recursive_install_without_a_filter_installs_every_python_project() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let _beta = serve(&mut server, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    requirements_project(root.path(), "a", &["alpha"]);
    requirements_project(root.path(), "b", &["beta"]);

    pacquet_in(root.path())
        .args(["install", "--recursive"])
        .assert()
        .success();

    assert!(installed(root.path(), "a"));
    assert!(installed(root.path(), "b"));
}

#[tokio::test]
async fn a_filter_that_selects_no_project_at_all_ends_the_install() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    requirements_project(root.path(), "a", &["alpha"]);

    assert_no_projects_matched(
        pacquet_in(root.path()).args(["install", "--filter", "missing", "--fail-if-no-match"]),
    );
    assert!(!installed(root.path(), "a"));
}

/// A Python project sharing a directory with an npm project follows that
/// project's selection: the workspace knows the directory by the npm
/// project's name, which is what a `--filter` names it by.
#[tokio::test]
async fn a_filter_scopes_a_python_project_by_the_npm_project_in_its_directory() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let _beta = serve(&mut server, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(root.path().join("pnpm-workspace.yaml"), format!("{workspace}packages:\n  - '*'\n"))
        .unwrap();
    requirements_project(root.path(), "web", &["alpha"]);
    requirements_project(root.path(), "api", &["beta"]);
    fs::write(root.path().join("web/package.json"), r#"{"name":"web","version":"1.0.0"}"#).unwrap();
    fs::write(
        root.path().join("web/pyproject.toml"),
        fs::read_to_string(root.path().join("web/pyproject.toml"))
            .unwrap()
            .replace("name = 'web'", "name = 'webapp'"),
    )
    .unwrap();

    pacquet_in(root.path())
        .args(["install", "--filter", "web"])
        .assert()
        .success();

    assert!(installed(root.path(), "web"));
    assert!(!installed(root.path(), "api"), "api was not selected");

    // The distribution the project declares does not name that directory:
    // the npm project in it does.
    assert_no_projects_matched(
        pacquet_in(root.path()).args(["install", "--filter", "webapp", "--fail-if-no-match"]),
    );

    pacquet_in(root.path())
        .args(["install", "--filter", "api"])
        .assert()
        .success();
    assert!(installed(root.path(), "api"));
}

/// A selected project installs the workspace projects it declares a source
/// for, so they take part in its resolution without an environment of their
/// own. Selecting its dependencies asks for those environments too.
#[tokio::test]
async fn a_filtered_install_links_the_workspace_sources_the_selected_project_declares() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    python_project(&root.path().join("packages/mylib"), "mylib", "dependencies = []");
    python_project(
        &root.path().join("packages/app"),
        "app",
        "dependencies = ['mylib']\n\n[tool.uv.sources]\nmylib = { workspace = true }\n",
    );

    pacquet_in(root.path())
        .args(["install", "--filter", "app"])
        .assert()
        .success();

    assert!(installed(root.path(), "packages/app"));
    assert!(!installed(root.path(), "packages/mylib"), "mylib was not selected");
    python(&root.path().join("packages/app"))
        .args(["-c", "import mylib; print(mylib.MARKER)"])
        .assert()
        .success()
        .stdout(if cfg!(windows) { "workspace mylib\r\n" } else { "workspace mylib\n" });

    pacquet_in(root.path())
        .args(["install", "--filter", "app..."])
        .assert()
        .success();
    assert!(installed(root.path(), "packages/mylib"));
}

#[tokio::test]
async fn a_filtered_add_writes_the_requirement_to_the_selected_project_only() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    requirements_project(root.path(), "a", &[]);
    requirements_project(root.path(), "b", &[]);

    pacquet_in(root.path())
        .args(["add", "--filter", "a", "pypi:alpha"])
        .assert()
        .success();

    let added = fs::read_to_string(root.path().join("a/pyproject.toml")).unwrap();
    assert!(added.contains("alpha>=1.0"), "pins what it resolved: {added}");
    assert!(
        !fs::read_to_string(root.path().join("b/pyproject.toml")).unwrap().contains("alpha"),
        "b was not selected",
    );
    assert!(installed(root.path(), "a"));
    assert!(!installed(root.path(), "b"), "b was not selected");
}

/// A recursive add leaves the workspace root out, the way the npm add does,
/// so a Python project at the root keeps its requirements.
#[tokio::test]
async fn a_recursive_add_leaves_the_workspace_root_alone() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!("{workspace}packages:\n  - '.'\n  - 'a'\n"),
    )
    .unwrap();
    requirements_project(root.path(), "a", &[]);

    pacquet_in(root.path())
        .args(["add", "--recursive", "pypi:alpha"])
        .assert()
        .success();

    assert!(fs::read_to_string(root.path().join("a/pyproject.toml")).unwrap().contains("alpha"),);
    assert!(
        !fs::read_to_string(root.path().join("pyproject.toml")).unwrap().contains("alpha"),
        "the root is not part of a recursive add",
    );
}

/// A project the workspace excludes is not reached through a member's
/// `[tool.uv.sources]` entry, the way resolving that entry would not reach
/// it either.
#[tokio::test]
async fn a_filter_does_not_reach_a_project_outside_the_declared_workspace() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    fs::write(
        root.path().join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = ['packages/*']\nexclude = ['packages/excluded']\n",
    )
    .unwrap();
    python_project(&root.path().join("packages/excluded"), "excluded", "dependencies = []");
    python_project(
        &root.path().join("packages/app"),
        "app",
        "dependencies = []\n\n[tool.uv.sources]\nexcluded = { workspace = true }\n",
    );

    pacquet_in(root.path())
        .args(["install", "--filter", "app..."])
        .assert()
        .success();

    assert!(installed(root.path(), "packages/app"));
    assert!(
        !installed(root.path(), "packages/excluded"),
        "the workspace excludes it, so app does not reach it",
    );
}
