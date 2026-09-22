//! Where a project's environment generations live: in the store, with the
//! project's `.venv` as its link to the generation it runs.

use super::{
    project,
    python,
    serve,
    wheel,
};
use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
};

/// The directory of every project the store under `root` holds
/// generations for, as the links pnpm publishes spell it.
pub(super) fn project_directories(root: &Path) -> Vec<PathBuf> {
    let Ok(projects) = fs::read_dir(root.join("store/v11/python-envs")) else {
        return Vec::new();
    };
    let mut directories = projects
        .map(|project| dunce::canonicalize(project.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

/// Every generation the store under `root` holds, for every project.
pub(super) fn generations(root: &Path) -> Vec<PathBuf> {
    let mut generations = project_directories(root)
        .into_iter()
        .flat_map(|project| fs::read_dir(project).unwrap())
        .map(|generation| generation.unwrap().path())
        .collect::<Vec<_>>();
    generations.sort();
    generations
}

fn environments_root(root: &Path) -> PathBuf {
    dunce::canonicalize(root.join("store/v11/python-envs")).unwrap()
}

/// The generation `project` links to, resolved, after checking that the
/// link holds it as an absolute path: the store is not beside the
/// project, and the link has to survive the project moving.
fn environment(project: &Path) -> PathBuf {
    let contents = pnpm_fs::read_symlink_dir(&project.join(".venv")).unwrap();
    assert!(contents.is_absolute(), "{}", contents.display());
    dunce::canonicalize(project.join(contents)).unwrap()
}

#[tokio::test]
async fn generations_live_in_the_store_and_the_project_holds_only_its_link() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let first = environment(root.path());
    eprintln!("environment: {}", first.display());
    assert_eq!(
        first.parent().and_then(Path::parent),
        Some(environments_root(root.path()).as_path()),
    );
    assert_eq!(generations(root.path()), vec![first.clone()]);
    assert!(!root.path().join(".pnpm").exists(), "the project holds a generation directory");
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();

    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    let second = environment(root.path());
    eprintln!("environment after the second install: {}", second.display());
    assert_ne!(second, first);
    assert_eq!(second.parent(), first.parent());
    let mut expected = vec![first, second];
    expected.sort();
    assert_eq!(generations(root.path()), expected);
}

#[tokio::test]
async fn sibling_projects_keep_their_generations_apart() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::write(root.path().join("pyproject.toml"), "[tool.ruff]\nline-length = 100\n").unwrap();
    for name in ["app-one", "app-two"] {
        let directory = root.path().join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("pyproject.toml"),
            "[project]\nname = 'app'\nversion = '1.0'\ndependencies = ['alpha>=1']\n",
        )
        .unwrap();
    }
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let one = environment(&root.path().join("app-one"));
    let two = environment(&root.path().join("app-two"));
    eprintln!("app-one: {}\napp-two: {}", one.display(), two.display());
    assert_ne!(one.parent(), two.parent());
    let environments = environments_root(root.path());
    for generation in [&one, &two] {
        assert_eq!(generation.parent().and_then(Path::parent), Some(environments.as_path()));
    }
    let mut expected = vec![one, two];
    expected.sort();
    assert_eq!(generations(root.path()), expected);
}

/// A project's `.venv` links into the store, so it stays pnpm's when the
/// project directory moves, and the install there replaces it rather than
/// refusing an environment it does not recognize.
#[tokio::test]
async fn a_moved_project_keeps_a_managed_environment() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let before = root.path().join("before");
    fs::create_dir(&before).unwrap();
    project(&before, &server.url(), &["alpha>=1"]);
    fs::write(
        before.join("pnpm-workspace.yaml"),
        format!(
            "python:\n  enabled: true\nregistries:\n  '{}/simple/':\n    ecosystem: pypi\n\
             storeDir: '{}'\ncacheDir: '{}'\nfetchRetries: 0\n",
            server.url(),
            root.path().join("store").display(),
            root.path().join("cache").display(),
        ),
    )
    .unwrap();
    pacquet_in(&before)
        .arg("install")
        .assert()
        .success();
    let previous = environment(&before);
    let after = root.path().join("after");
    fs::rename(&before, &after).unwrap();
    assert_eq!(environment(&after), previous, "the link survives the move");
    pacquet_in(&after)
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    let current = environment(&after);
    eprintln!("before the move: {}\nafter the move: {}", previous.display(), current.display());
    assert_ne!(current.parent(), previous.parent());
    assert_eq!(
        current.parent().and_then(Path::parent),
        Some(environments_root(root.path()).as_path()),
    );
    assert!(previous.is_dir(), "the generation a running program may use was removed");
    python(&after)
        .args(["-c", "import alpha"])
        .assert()
        .success();
}

/// A checkout can carry a `.pnpm` entry pointing anywhere; an install has
/// no reason to touch it.
#[cfg(unix)]
#[tokio::test]
async fn an_install_writes_nothing_into_the_project_pnpm_directory() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    std::os::unix::fs::symlink(outside.path(), root.path().join(".pnpm")).unwrap();
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert_eq!(generations(root.path()).len(), 1);
}

/// A project pnpm 12.4 installed links to a generation in its own
/// `.pnpm/python-envs`.
#[tokio::test]
async fn an_environment_beside_the_project_is_replaced_by_one_in_the_store() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    let beside = root.path().join(".pnpm/python-envs/env-old");
    fs::create_dir_all(&beside).unwrap();
    fs::write(beside.join("pyvenv.cfg"), "").unwrap();
    pnpm_fs::force_symlink_dir(&beside, &root.path().join(".venv")).unwrap();
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let current = environment(root.path());
    eprintln!("environment: {}", current.display());
    assert_eq!(
        current.parent().and_then(Path::parent),
        Some(environments_root(root.path()).as_path()),
    );
    assert!(
        beside.join("pyvenv.cfg").is_file(),
        "the generation a running program may use was removed",
    );
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();
}

/// The store a `.venv` links into can be removed on its own now, and a
/// link to nothing is nothing to protect.
#[tokio::test]
async fn a_dangling_link_is_replaced() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    let gone = root.path().join("store/v11/python-envs/project/env-gone");
    fs::create_dir_all(&gone).unwrap();
    pnpm_fs::force_symlink_dir(&gone, &root.path().join(".venv")).unwrap();
    fs::remove_dir_all(root.path().join("store")).unwrap();
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let current = environment(root.path());
    eprintln!("environment: {}", current.display());
    assert!(current.is_dir());
    assert_ne!(current, gone);
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();
}

/// `frozenStore` promises that nothing under the store is written, so a
/// project installed from one keeps its generations beside it.
#[tokio::test]
async fn a_frozen_store_keeps_generations_beside_the_project() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let in_store = environment(root.path());
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile", "--frozen-store"])
        .assert()
        .success();
    let beside = environment(root.path());
    eprintln!("environment: {}", beside.display());
    assert_eq!(
        beside.parent(),
        Some(dunce::canonicalize(root.path().join(".pnpm/python-envs")).unwrap().as_path()),
    );
    assert_eq!(generations(root.path()), vec![in_store], "the frozen store was written");
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();
}
