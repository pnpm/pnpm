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

    // A member asking both ranges does not hide the other member asking
    // one of them.
    python_project(
        &root.path().join("packages/a"),
        "a",
        "dependencies = ['alpha==1.0', 'alpha==2.0']",
    );
    python_project(&root.path().join("packages/b"), "b", "dependencies = ['alpha==1.0']");
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        &format!(
            "{} requires `alpha==1.0` and {} requires `alpha==2.0`",
            packages.join("b").display(),
            packages.join("a").display(),
        ),
    );
}

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

#[tokio::test]
async fn a_shared_member_with_dynamic_metadata_is_prepared_with_the_shared_interpreter() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    shared_workspace(root.path(), &server.url());
    python_project(&root.path().join("packages/lib"), "lib", "dependencies = ['alpha']");
    let app = root.path().join("packages/app");
    fs::create_dir_all(app.join("app")).unwrap();
    fs::write(app.join("app/__init__.py"), "VALUE = 'source'\n").unwrap();
    fs::write(
        app.join("pyproject.toml"),
        "[project]\nname = 'app'\ndynamic = ['version']\nrequires-python = '>=3.10'\n\
         dependencies = ['lib']\n\n[tool.uv.sources]\nlib = { workspace = true }\n\n\
         [build-system]\nrequires = ['hatchling']\nbuild-backend = 'hatchling.build'\n",
    )
    .unwrap();

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    python(root.path())
        .args(["-c", "import alpha, lib; import importlib.metadata as m; print(m.version('app'))"])
        .assert()
        .success()
        .stdout(line("0.0.1"));
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pylock.toml")).unwrap()).unwrap();
    assert_eq!(lock["tool"]["pnpm"]["members"], list(&["packages/app", "packages/lib"]));
}

/// An interpreter on the PATH under `name` that records each start in
/// `log`, so a test can tell which interpreters an install ran.
#[cfg(unix)]
fn logging_shim(directory: &Path, name: &str, version: &str, log: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(directory).unwrap();
    let source = super::interpreter_shim_source(version)
        .replace(
            "import platform, sys\n",
            &format!(
                "import os, platform, sys\nopen('{}', 'a').write('{name}\\n')\n",
                log.display(),
            ),
        );
    fs::write(directory.join(name), source).unwrap();
    fs::set_permissions(directory.join(name), fs::Permissions::from_mode(0o755)).unwrap();
}

/// The member asks for Python 3.12.7 in its own `.python-version`, which
/// would select it for the member's dynamic metadata on its own. Sharing
/// an environment, the metadata is prepared with the interpreter the
/// root asks for, and the member's own choice is never started. The pins
/// name patch releases so that a host interpreter of the same minor
/// version is not taken before the shim.
#[cfg(unix)]
#[tokio::test]
async fn a_shared_members_metadata_is_prepared_with_the_interpreter_the_root_asks_for() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    shared_workspace(root.path(), &server.url());
    fs::write(root.path().join(".python-version"), "3.11.9\n").unwrap();
    python_project(&root.path().join("packages/lib"), "lib", "dependencies = ['alpha']");
    let app = root.path().join("packages/app");
    fs::create_dir_all(app.join("app")).unwrap();
    fs::write(app.join("app/__init__.py"), "VALUE = 'source'\n").unwrap();
    fs::write(
        app.join("pyproject.toml"),
        "[project]\nname = 'app'\ndynamic = ['version']\nrequires-python = '>=3.10'\n\
         dependencies = []\n\n[build-system]\nrequires = ['hatchling']\n\
         build-backend = 'hatchling.build'\n",
    )
    .unwrap();
    fs::write(app.join(".python-version"), "3.12.7\n").unwrap();
    // Outside the workspace, which an install does not take interpreters from.
    let outside = tempfile::tempdir().unwrap();
    let shims = outside.path().join("interpreters");
    let log = outside.path().join("started");
    logging_shim(&shims, "python3.11", "3.11.9", &log);
    logging_shim(&shims, "python3.12", "3.12.7", &log);

    pacquet_in(root.path())
        .arg("install")
        .env("PATH", format!("{}:{}", shims.display(), std::env::var("PATH").unwrap()))
        .assert()
        .success();

    assert_eq!(super::selected_python(root.path()), "3.11.9");
    let started = fs::read_to_string(&log).unwrap();
    eprintln!("started:\n{started}");
    assert!(started.contains("python3.11"), "the shared interpreter prepared the metadata");
    assert!(!started.contains("python3.12"), "the member's own choice was never started");
    python(root.path())
        .args(["-c", "import alpha, lib; import importlib.metadata as m; print(m.version('app'))"])
        .assert()
        .success()
        .stdout(line("0.0.1"));
}

#[tokio::test]
async fn an_add_outside_the_declared_workspace_reads_the_project_alone() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    let plain = |name: &str| {
        format!(
            "[project]\nname = '{name}'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
             dependencies = []\n",
        )
    };
    for (directory, manifest) in [
        (
            "api",
            "[tool.uv.workspace]\nmembers = ['providers/*']\n\n[tool.pnpm.python]\n\
         shared-environment = true\n"
                .to_string(),
        ),
        ("api/providers/good", plain("good")),
        ("api/providers/broken", "[project\n".to_string()),
        ("tools/x", plain("x")),
    ] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
        fs::write(
            root.path()
                .join(directory)
                .join("pyproject.toml"),
            manifest,
        )
        .unwrap();
    }
    let outside = root.path().join("tools/x");

    pacquet_in(&outside)
        .args(["add", "pypi:alpha"])
        .assert()
        .success();

    assert!(fs::read_to_string(outside.join("pyproject.toml")).unwrap().contains("alpha>=1.0"));
    assert!(outside.join("pylock.toml").is_file(), "an environment of its own");
    assert!(
        !root
            .path()
            .join("api/pylock.toml")
            .exists(),
        "the workspace was not installed",
    );
    pacquet_in(&root.path().join("api/providers/good"))
        .args(["add", "pypi:alpha"])
        .assert()
        .failure();
}
