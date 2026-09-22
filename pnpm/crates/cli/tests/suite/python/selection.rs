use super::{
    project,
    python,
    serve,
    wheel,
};
use assert_cmd::prelude::*;
use std::fs;

#[tokio::test]
async fn workspace_selections_apply_only_to_projects_that_define_them() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let mut mocks = Vec::new();
    for name in ["alpha", "beta", "gamma", "delta"] {
        mocks.extend(serve(&mut server, name, &[("1.0", wheel(name, "1.0", "", &[]))]).await);
    }
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    super::add_python_settings(root.path(), "  extras: [cli, web]\n  groups: [dev, test]\n");
    for (name, extra, optional, group, development) in
        [("a", "cli", "alpha", "dev", "beta"), ("b", "web", "gamma", "test", "delta")]
    {
        let directory = root.path().join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("pyproject.toml"), format!("[project]\nname = '{name}'\nversion = '1.0'\ndependencies = []\n[project.optional-dependencies]\n{extra} = ['{optional}']\n[dependency-groups]\n{group} = ['{development}']\n")).unwrap();
    }
    super::pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(&root.path().join("a")).args(["-c", "import alpha, beta; import importlib.util; assert importlib.util.find_spec('gamma') is None"]).assert().success();
    python(&root.path().join("b")).args(["-c", "import gamma, delta; import importlib.util; assert importlib.util.find_spec('alpha') is None"]).assert().success();
}

#[tokio::test]
async fn project_overrides_change_lock_inputs_and_preserve_production_projection() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let _beta = serve(&mut server, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    let _gamma = serve(&mut server, "gamma", &[("1.0", wheel("gamma", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    super::add_python_settings(root.path(), "  extras: [cli]\n  groups: [test]\n");
    let manifest = "[project]\nname = 'app'\nversion = '1.0'\ndependencies = []\n[project.optional-dependencies]\ncli = ['alpha']\nweb = ['beta']\n[dependency-groups]\ntest = ['alpha']\ndev = ['gamma']\n[tool.pnpm.python]\nextras = ['web']\ngroups = ['dev']\n";
    fs::write(root.path().join("pyproject.toml"), manifest).unwrap();
    super::pacquet_in(root.path())
        .args(["install", "--lockfile-only"])
        .assert()
        .success();
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    super::pacquet_in(root.path())
        .args(["install", "--prod", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path()).args(["-c", "import beta; import importlib.util; assert importlib.util.find_spec('alpha') is None; assert importlib.util.find_spec('gamma') is None"]).assert().success();
    assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
    super::pacquet_in(root.path())
        .args(["install", "--dev", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path()).args(["-c", "import gamma; import importlib.util; assert importlib.util.find_spec('alpha') is None; assert importlib.util.find_spec('beta') is None"]).assert().success();
    assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
    fs::write(
        root.path().join("pyproject.toml"),
        manifest.replace("extras = ['web']", "extras = []"),
    )
    .unwrap();
    super::assert_failure_contains(
        super::pacquet_in(root.path()).args(["install", "--frozen-lockfile"]),
        "frozen",
    );
}
