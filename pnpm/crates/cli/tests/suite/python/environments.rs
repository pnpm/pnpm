//! Where a project's environment generations live: in the store, with the
//! project's `.venv` as its link to the generation it runs.
use command_extra::CommandExtra;

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
        .map(Result::unwrap)
        .map(|value| value.path())
        .map(dunce::canonicalize)
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

/// Every generation the store under `root` holds, for every project.
pub(super) fn generations(root: &Path) -> Vec<PathBuf> {
    let mut generations = project_directories(root)
        .into_iter()
        .map(fs::read_dir)
        .flat_map(Result::unwrap)
        .map(Result::unwrap)
        .filter(|value| {
            value
                .file_name()
                .to_string_lossy()
                .starts_with("env-")
        })
        .map(|value| value.path())
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
        .with_arg("install")
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
        .with_args(["-c", "import alpha"])
        .assert()
        .success();

    pacquet_in(root.path())
        .with_args(["install", "--offline", "--frozen-lockfile"])
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
async fn store_prune_removes_the_generations_the_project_does_not_run() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path())
        .with_arg("install")
        .assert()
        .success();
    pacquet_in(root.path())
        .with_args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    let current = environment(root.path());
    assert_eq!(generations(root.path()).len(), 2);

    let output = pacquet_in(root.path())
        .with_args(["store", "prune"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}");
    assert!(output.status.success());
    assert!(stderr.contains("Removed 1 unused Python environment\n"), "{stderr}");
    assert_eq!(generations(root.path()), vec![current]);
    python(root.path())
        .with_args(["-c", "import alpha"])
        .assert()
        .success();
}

#[tokio::test]
/// A moved project's `.venv` still links to its newest generation, while the
/// store's link back to the project names the old location.
async fn store_prune_keeps_the_generation_a_moved_project_runs() {
    let root = tempfile::tempdir().unwrap();
    let project_dir = root.path().join("app");
    fs::create_dir_all(&project_dir).unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(&project_dir, &server.url(), &["alpha>=1"]);
    let store = root.path().join("store");
    let manifest = project_dir.join("pnpm-workspace.yaml");
    let settings = fs::read_to_string(&manifest)
        .unwrap()
        .replace(
            &project_dir
                .join("store")
                .display()
                .to_string(),
            &store.display().to_string(),
        );
    fs::write(&manifest, settings).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!("storeDir: '{}'\n", store.display()),
    )
    .unwrap();
    pacquet_in(&project_dir)
        .with_arg("install")
        .assert()
        .success();
    pacquet_in(&project_dir)
        .with_args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    let current = environment(&project_dir);
    assert_eq!(generations(root.path()).len(), 2);
    let moved = root.path().join("moved");
    fs::rename(&project_dir, &moved).unwrap();

    let output = pacquet_in(root.path())
        .with_args(["store", "prune"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}");
    assert!(output.status.success());
    assert_eq!(generations(root.path()), vec![current]);
    python(&moved)
        .with_args(["-c", "import alpha"])
        .assert()
        .success();
}

/// A project directory an older pnpm made has no link back to its project,
/// so nothing tells which of its generations is in use.
#[tokio::test]
async fn store_prune_keeps_the_generations_of_a_project_directory_without_a_link_back() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path())
        .with_arg("install")
        .assert()
        .success();
    let current = environment(root.path());
    let directory = current.parent().unwrap();
    pnpm_fs::remove_symlink_dir(&directory.join("project")).unwrap();
    let older = directory.join("env-older");
    fs::create_dir(&older).unwrap();

    pacquet_in(root.path())
        .with_args(["store", "prune"])
        .assert()
        .success();
    let mut expected = vec![current, older];
    expected.sort();
    assert_eq!(generations(root.path()), expected);
}

/// An environment rewrites each wheel's `RECORD`, so the store copy has no
/// hard link while the environment still uses the wheel. The store is
/// inside the project, which an npm project would not register.
#[tokio::test]
async fn store_prune_keeps_the_wheels_of_an_installed_project() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path())
        .with_arg("install")
        .assert()
        .success();

    let output = pacquet_in(root.path())
        .with_args(["store", "prune"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}");
    assert!(output.status.success());
    assert!(stderr.contains("Removed 0 files"), "{stderr}");

    pnpm_fs::remove_symlink_dir(&root.path().join(".venv")).unwrap();
    pacquet_in(root.path())
        .with_args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path())
        .with_args(["-c", "import alpha"])
        .assert()
        .success();
}

#[tokio::test]
async fn store_prune_stops_on_a_malformed_lockfile_without_quoting_it() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path())
        .with_arg("install")
        .assert()
        .success();
    fs::write(root.path().join("pylock.toml"), "//registry.test/:_authToken=secret-token\n")
        .unwrap();

    let output = pacquet_in(root.path())
        .with_args(["store", "prune"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}");
    assert!(!output.status.success());
    assert!(stderr.contains("not a valid pylock.toml"), "{stderr}");
    assert!(!stderr.contains("secret-token"), "{stderr}");
    assert!(!stderr.contains("Removed"), "{stderr}");
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
        .with_arg("install")
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
        .with_arg("install")
        .assert()
        .success();
    let previous = environment(&before);
    let after = root.path().join("after");
    fs::rename(&before, &after).unwrap();
    assert_eq!(environment(&after), previous, "the link survives the move");
    pacquet_in(&after)
        .with_args(["install", "--offline", "--frozen-lockfile"])
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
        .with_args(["-c", "import alpha"])
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
        .with_arg("install")
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
        .with_arg("install")
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
        .with_args(["-c", "import alpha"])
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
        .with_arg("install")
        .assert()
        .success();
    let current = environment(root.path());
    eprintln!("environment: {}", current.display());
    assert!(current.is_dir());
    assert_ne!(current, gone);
    python(root.path())
        .with_args(["-c", "import alpha"])
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
        .with_arg("install")
        .assert()
        .success();
    let in_store = environment(root.path());
    pacquet_in(root.path())
        .with_args(["install", "--offline", "--frozen-lockfile", "--frozen-store"])
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
        .with_args(["-c", "import alpha"])
        .assert()
        .success();
}

#[tokio::test]
async fn outdated_reports_divergent_platform_locked_versions() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let alpha_index = server
        .mock("GET", "/simple/alpha/")
        .with_status(200)
        .with_header("content-type", "application/vnd.pypi.simple.v1+json")
        .with_body(
            serde_json::json!({
                "meta": { "api-version": "1.0" },
                "name": "alpha",
                "files": [
                    { "filename": "alpha-1.0-py3-none-any.whl", "url": "https://example.test/alpha-1.0-py3-none-any.whl", "hashes": { "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } },
                    { "filename": "alpha-2.0-py3-none-any.whl", "url": "https://example.test/alpha-2.0-py3-none-any.whl", "hashes": { "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" } },
                    { "filename": "alpha-3.0-py3-none-any.whl", "url": "https://example.test/alpha-3.0-py3-none-any.whl", "hashes": { "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc" } },
                ],
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let beta_index = server
        .mock("GET", "/simple/beta/")
        .with_status(200)
        .with_header("content-type", "application/vnd.pypi.simple.v1+json")
        .with_body(
            serde_json::json!({
                "meta": { "api-version": "1.0" },
                "name": "beta",
                "files": [
                    { "filename": "beta-1.0-py3-none-any.whl", "url": "https://example.test/beta-1.0-py3-none-any.whl", "hashes": { "sha256": "1111111111111111111111111111111111111111111111111111111111111111" } },
                    { "filename": "beta-2.0-py3-none-any.whl", "url": "https://example.test/beta-2.0-py3-none-any.whl", "hashes": { "sha256": "2222222222222222222222222222222222222222222222222222222222222222" } },
                    { "filename": "beta-3.0-py3-none-any.whl", "url": "https://example.test/beta-3.0-py3-none-any.whl", "hashes": { "sha256": "3333333333333333333333333333333333333333333333333333333333333333" } },
                ],
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    project(
        root.path(),
        &server.url(),
        &[
            "alpha>=1",
            "beta>=1,<3 ; sys_platform == 'linux'",
            "beta==1.0 ; sys_platform == 'win32'",
        ],
    );
    std::fs::write(
        root.path().join("pylock.toml"),
        concat!(
            "lock-version = '1.0'\n",
            "environments = [\"sys_platform == 'linux'\", \"sys_platform == 'win32'\"]\n",
            "created-by = 'pnpm'\n",
            "[[packages]]\nname = 'alpha'\nversion = '1.0'\nmarker = \"sys_platform == 'linux'\"\n",
            "wheels = [{ name = 'alpha-1.0-py3-none-any.whl', url = 'https://example.test/alpha-1.0-py3-none-any.whl', hashes = { sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' } }]\n",
            "[[packages]]\nname = 'alpha'\nversion = '2.0'\nmarker = \"sys_platform == 'win32'\"\n",
            "wheels = [{ name = 'alpha-2.0-py3-none-any.whl', url = 'https://example.test/alpha-2.0-py3-none-any.whl', hashes = { sha256 = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' } }]\n",
            "[[packages]]\nname = 'beta'\nversion = '1.0'\n",
            "wheels = [{ name = 'beta-1.0-py3-none-any.whl', url = 'https://example.test/beta-1.0-py3-none-any.whl', hashes = { sha256 = '1111111111111111111111111111111111111111111111111111111111111111' } }]\n",
        ),
    )
    .unwrap();
    let output = pacquet_in(root.path())
        .with_args(["outdated", "pypi:*", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    alpha_index.assert_async().await;
    beta_index.assert_async().await;
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["alpha@1.0"]["current"], "1.0");
    assert_eq!(report["alpha@1.0"]["latest"], "3.0");
    assert_eq!(report["alpha@2.0"]["current"], "2.0");
    assert_eq!(report["alpha@2.0"]["latest"], "3.0");
    assert_eq!(report["beta"]["current"], "1.0");
    assert_eq!(report["beta"]["wanted"], "2.0");
    assert_eq!(report["beta"]["latest"], "3.0");
}
