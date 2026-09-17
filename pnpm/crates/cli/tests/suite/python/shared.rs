//! A workspace whose root asks for one environment shared by its members.

use super::{
    assert_failure_contains, project, python, python_project, serve, serve_backends, wheel,
};
use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use std::{fs, path::Path};

fn shared_workspace(root: &Path, index: &str) {
    project(root, index, &[]);
    fs::write(
        root.join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = ['packages/*']\n\n[tool.pnpm.python]\n\
         shared-environment = true\n",
    )
    .unwrap();
}

fn has_own_environment(root: &Path) -> bool {
    root.join("pylock.toml").exists() || root.join(".venv").exists()
}

fn list(items: &[&str]) -> toml::Value {
    toml::Value::Array(
        items
            .iter()
            .map(|item| toml::Value::String(item.to_string()))
            .collect(),
    )
}

fn line(text: &str) -> String {
    format!("{text}{}", if cfg!(windows) { "\r\n" } else { "\n" })
}

#[tokio::test]
async fn members_of_a_shared_workspace_install_into_one_environment() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let _beta = serve(&mut server, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    shared_workspace(root.path(), &server.url());
    python_project(&root.path().join("packages/lib"), "lib", "dependencies = ['alpha']");
    python_project(&root.path().join("packages/app"), "app", "dependencies = ['beta', 'lib']");
    let app = root.path().join("packages/app");
    let manifest = fs::read_to_string(app.join("pyproject.toml")).unwrap();
    fs::write(
        app.join("pyproject.toml"),
        format!(
            "{}\n[tool.uv.sources]\nlib = {{ workspace = true }}\n",
            manifest.replace("requires-python = '>=3.10'", "requires-python = '>=3.10,<4'"),
        ),
    )
    .unwrap();

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("{lock}");
    let recorded: toml::Value = toml::from_str(&lock).unwrap();
    assert_eq!(recorded["tool"]["pnpm"]["members"], list(&["packages/app", "packages/lib"]));
    assert_eq!(recorded["requires-python"].as_str(), Some(">=3.10, <4"));
    assert!(lock.contains(r#"path = "packages/lib""#), "{lock}");
    assert!(!has_own_environment(&app), "app shares the workspace environment");
    assert!(!has_own_environment(&root.path().join("packages/lib")), "lib shares it too");
    python(root.path())
        .args(["-c", "import alpha, beta, lib, app; print(lib.MARKER, app.MARKER)"])
        .assert()
        .success()
        .stdout(line("workspace lib workspace app"));
    // A command run in a member finds the environment its workspace shares.
    pacquet_in(&app)
        .args(["exec", "python", "-c", "import alpha, app; print(app.MARKER)"])
        .assert()
        .success()
        .stdout(line("workspace app"));

    pnpm_fs::remove_symlink_dir(&root.path().join(".venv")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha, beta, lib, app"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
}

#[tokio::test]
async fn members_that_cannot_be_installed_together_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "", &[])), ("2.0", wheel("alpha", "2.0", "", &[]))],
    )
    .await;
    shared_workspace(root.path(), &server.url());
    python_project(&root.path().join("packages/a"), "a", "dependencies = ['alpha==1.0']");
    python_project(&root.path().join("packages/b"), "b", "dependencies = ['alpha==2.0']");

    // The projects are named by the path pnpm read them at, which is the
    // canonical one where the temporary directory is reached through a link.
    let packages = dunce::canonicalize(root.path()).unwrap().join("packages");
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        &format!(
            "cannot be installed together: {} requires `alpha==1.0` and {} requires `alpha==2.0`, \
             and no version of alpha the index offers satisfies both",
            packages.join("a").display(),
            packages.join("b").display(),
        ),
    );
    assert!(!root.path().join("pylock.toml").exists());
}

/// A shared environment is one thing, so selecting a member installs it
/// whole rather than an environment holding that member alone.
#[tokio::test]
async fn selecting_one_member_installs_the_environment_they_share() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let _beta = serve(&mut server, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    shared_workspace(root.path(), &server.url());
    python_project(&root.path().join("packages/a"), "a", "dependencies = ['alpha']");
    python_project(&root.path().join("packages/b"), "b", "dependencies = ['beta']");

    pacquet_in(root.path())
        .args(["install", "--filter", "a", "--fail-if-no-match"])
        .assert()
        .success();

    python(root.path())
        .args(["-c", "import alpha, beta, a, b"])
        .assert()
        .success();
    assert!(!has_own_environment(&root.path().join("packages/a")));
}

#[tokio::test]
async fn adding_to_a_member_updates_its_manifest_and_the_shared_lockfile() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let _gamma = serve(&mut server, "gamma", &[("1.0", wheel("gamma", "1.0", "", &[]))]).await;
    shared_workspace(root.path(), &server.url());
    python_project(&root.path().join("packages/a"), "a", "dependencies = ['alpha']");
    python_project(&root.path().join("packages/b"), "b", "dependencies = []");

    pacquet_in(&root.path().join("packages/b"))
        .args(["add", "pypi:gamma"])
        .assert()
        .success();

    let edited = fs::read_to_string(root.path().join("packages/b/pyproject.toml")).unwrap();
    assert!(edited.contains("gamma>=1.0"), "pins what it resolved: {edited}");
    assert!(
        !fs::read_to_string(root.path().join("packages/a/pyproject.toml"))
            .unwrap()
            .contains("gamma"),
        "a was not edited",
    );
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pylock.toml")).unwrap()).unwrap();
    assert_eq!(lock["tool"]["pnpm"]["requirements"], list(&["alpha", "gamma>=1.0"]));
    assert_eq!(lock["tool"]["pnpm"]["members"], list(&["packages/a", "packages/b"]));
    assert!(!has_own_environment(&root.path().join("packages/b")));
    python(root.path())
        .args(["-c", "import alpha, gamma, a, b"])
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
}
