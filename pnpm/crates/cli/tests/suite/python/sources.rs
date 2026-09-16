mod backtracking;
mod compatibility;
mod frozen;

use super::{
    assert_failure_contains, project, python, python_project, serve, serve_backends, wheel,
};
use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, fs, path::Path, process::Command};
use url::Url;

#[tokio::test]
async fn direct_and_uv_url_wheels_are_hashed_and_replayed_offline() {
    for uv in [false, true] {
        eprintln!("uv source: {uv}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let archive = wheel("alpha", "1.0", "Requires-Dist: helper>=1\n", &[]);
        let digest = format!("{:x}", Sha256::digest(&archive));
        let artifact = server
            .mock("GET", "/files/alpha-1.0-py3-none-any.whl")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_body(archive)
            .expect(1)
            .create_async()
            .await;
        let index = server
            .mock("GET", "/simple/alpha/")
            .expect(0)
            .create_async()
            .await;
        let _helper =
            serve(&mut server, "helper", &[("1.0", wheel("helper", "1.0", "", &[]))]).await;
        let url = format!("{}/files/alpha-1.0-py3-none-any.whl", server.url());
        if uv {
            project(root.path(), &server.url(), &["alpha>=1"]);
            let manifest = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
            fs::write(
                root.path().join("pyproject.toml"),
                format!("{manifest}\n[tool.uv.sources]\nalpha = {{ url = '{url}' }}\n"),
            )
            .unwrap();
        } else {
            project(root.path(), &server.url(), &[&format!("alpha @ {url}")]);
        }
        declare_platforms(root.path());
        pacquet_in(root.path())
            .arg("install")
            .assert()
            .success();
        let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        assert!(lock.contains(&digest), "lock: {lock}");
        python(root.path()).args(["-c", "import alpha, helper; import importlib.metadata as m, json; print(json.loads(m.distribution('alpha').read_text('direct_url.json'))['archive_info']['hashes']['sha256'])"])
            .assert().success().stdout(format!("{digest}{}", if cfg!(windows) { "\r\n" } else { "\n" }));
        artifact.assert_async().await;
        index.assert_async().await;
        pnpm_fs::remove_symlink_dir(&root.path().join(".venv")).unwrap();
        pacquet_in(root.path())
            .args(["install", "--offline", "--frozen-lockfile"])
            .assert()
            .success();
        python(root.path())
            .args(["-c", "import alpha, helper"])
            .assert()
            .success();
        assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
    }
}

#[tokio::test]
async fn direct_wheel_hashes_and_identities_are_verified() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let archive = wheel("alpha", "1.0", "", &[]);
    let artifact = server
        .mock("GET", "/alpha-1.0-py3-none-any.whl")
        .with_body(&archive)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/alpha-1.0-py3-none-any.whl", server.url());
    project(root.path(), &server.url(), &[&format!("alpha @ {url}#sha256={}", "0".repeat(64))]);
    assert_failure_contains(pacquet_in(root.path()).arg("install"), "integrity");
    artifact.assert_async().await;
    let digest = format!("{:x}", Sha256::digest(&archive));
    project(root.path(), &server.url(), &[&format!("other @ {url}#sha256={digest}")]);
    assert_failure_contains(pacquet_in(root.path()).arg("install"), "names a wheel of alpha");
    assert!(!root.path().join("pylock.toml").exists(), "failed install published a lockfile");
}

#[tokio::test]
async fn a_transitive_direct_wheel_is_not_resolved_from_the_index() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let direct = format!("{}/helper-1.0-py3-none-any.whl", server.url());
    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", &format!("Requires-Dist: helper @ {direct}\n"), &[]))],
    )
    .await;
    let artifact = server
        .mock("GET", "/helper-1.0-py3-none-any.whl")
        .with_body(wheel("helper", "1.0", "", &[]))
        .expect(1)
        .create_async()
        .await;
    project(root.path(), &server.url(), &["alpha"]);
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha, helper"])
        .assert()
        .success();
    artifact.assert_async().await;
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "commit.gpgSign=false", "-c", "tag.gpgSign=false"])
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .to_string()
}

fn repository(root: &Path) -> (String, String) {
    python_project(root, "fork", "dependencies = []");
    git(root, &["init", "-b", "main"]);
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
            "initial",
        ],
    );
    let commit = git(root, &["rev-parse", "HEAD"]);
    git(root, &["tag", "v1"]);
    git(root, &["tag", "v1#fork"]);
    (Url::from_directory_path(root).unwrap().to_string(), commit)
}

fn approve(root: &Path, name: &str) {
    let config = root.join("pnpm-workspace.yaml");
    let mut contents = fs::read_to_string(&config).unwrap();
    writeln!(contents, "  pkg:pypi/{name}: true").unwrap();
    fs::write(config, contents).unwrap();
}

#[tokio::test]
async fn git_revisions_and_direct_requirements_pin_commits_and_replay_offline() {
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    for (field, revision, direct) in [
        ("rev", &commit[..7], false),
        ("tag", "v1", false),
        ("branch", "main", false),
        ("tag", "v1#fork", false),
        ("rev", commit.as_str(), true),
    ] {
        eprintln!("git source: {field} {revision}, direct: {direct}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _backends = serve_backends(&mut server).await;
        let index = server
            .mock("GET", "/simple/fork/")
            .expect(0)
            .create_async()
            .await;
        project(root.path(), &server.url(), &["fork>=1"]);
        approve(root.path(), "fork");
        let manifest = if direct {
            format!(
                "[project]\nname='app'\nversion='1.0'\ndependencies=['fork @ git+{url}@{revision}']\n",
            )
        } else {
            let manifest = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
            format!(
                "{manifest}\n[tool.uv.sources]\nfork = {{ git = '{url}', {field} = '{revision}' }}\n",
            )
        };
        fs::write(root.path().join("pyproject.toml"), &manifest).unwrap();
        declare_platforms(root.path());
        let trace = root.path().join("git-trace");
        pacquet_in(root.path())
            .env("GIT_TRACE", &trace)
            .arg("install")
            .assert()
            .success();
        let invocations = fs::read_to_string(&trace).unwrap();
        assert_eq!(invocations.matches("built-in: git clone").count(), 1, "{invocations}");
        let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        assert!(lock.contains(&format!(r#"commit-id = "{commit}""#)), "lock: {lock}");
        assert!(!lock.contains("packages.directory"), "git source locked as a directory: {lock}");
        python(root.path()).args(["-c", "import fork, importlib.metadata as m, json; print(fork.MARKER); print(json.loads(m.distribution('fork').read_text('direct_url.json'))['vcs_info']['commit_id'])"])
            .assert().success().stdout(format!("workspace fork{newline}{commit}{newline}", newline=if cfg!(windows) { "\r\n" } else { "\n" }));
        pnpm_fs::remove_symlink_dir(&root.path().join(".venv")).unwrap();
        pacquet_in(root.path())
            .env("GIT_ALLOW_PROTOCOL", "https")
            .args(["install", "--offline", "--frozen-lockfile"])
            .assert()
            .success();
        assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
        index.assert_async().await;
        fs::write(root.path().join("pyproject.toml"), manifest.replace(revision, "different"))
            .unwrap();
        assert_failure_contains(
            pacquet_in(root.path()).args(["install", "--offline", "--frozen-lockfile"]),
            "requirements changed",
        );
    }
}

#[tokio::test]
async fn a_git_dependency_is_not_built_without_its_own_approval() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &[&format!("fork @ git+{url}@{commit}")]);
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "requires approval of pkg:pypi/fork",
    );
}

#[tokio::test]
async fn inactive_url_requirements_are_not_fetched() {
    let root = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    project(
        root.path(),
        &server.url(),
        &["alpha @ https://example.invalid/alpha-1.0-py3-none-any.whl ; sys_platform == 'never'"],
    );
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
}

#[tokio::test]
async fn transitive_urls_selected_by_extras_replay_from_the_lockfile() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let url = format!("{}/helper-1.0-py3-none-any.whl", server.url());
    let _alpha = serve(
        &mut server,
        "alpha",
        &[(
            "1.0",
            wheel(
                "alpha",
                "1.0",
                &format!(
                    "Provides-Extra: helpers\nRequires-Dist: helper @ {url} ; extra == 'helpers'\n",
                ),
                &[],
            ),
        )],
    )
    .await;
    let artifact = server
        .mock("GET", "/helper-1.0-py3-none-any.whl")
        .with_body(wheel("helper", "1.0", "", &[]))
        .expect(1)
        .create_async()
        .await;
    project(root.path(), &server.url(), &["alpha[helpers]"]);
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path()).args(["-c", "import alpha, helper; import importlib.metadata as m, json; assert json.loads(m.distribution('helper').read_text('direct_url.json'))['archive_info']['hashes']['sha256']"]).assert().success();
    artifact.assert_async().await;
}

#[tokio::test]
async fn a_git_subdirectory_stays_at_its_locked_commit_when_the_branch_moves() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let (url, _) = repository(repo.path());
    fs::create_dir(repo.path().join("python")).unwrap();
    fs::rename(repo.path().join("pyproject.toml"), repo.path().join("python/pyproject.toml"))
        .unwrap();
    fs::rename(repo.path().join("src"), repo.path().join("python/src")).unwrap();
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "subdirectory",
        ],
    );
    let commit = git(repo.path(), &["rev-parse", "HEAD"]);
    project(root.path(), &server.url(), &[&format!("fork @ git+{url}@main#subdirectory=python")]);
    approve(root.path(), "fork");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    fs::write(repo.path().join("python/src/fork/__init__.py"), "MARKER = 'moved'\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "move"],
    );
    pacquet_in(root.path())
        .args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import fork; assert fork.MARKER == 'workspace fork'"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
    drop(repo);
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    fs::write(root.path().join("pylock.toml"), lock.replace(&commit, "main")).unwrap();
    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--offline", "--frozen-lockfile"]),
        "requires a full commit hash",
    );
}

#[tokio::test]
async fn git_subdirectories_cannot_escape_the_repository() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &[&format!("fork @ git+{url}@{commit}#subdirectory=..")]);
    approve(root.path(), "fork");
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "subdirectory must be relative and stay inside",
    );
}

#[tokio::test]
async fn git_version_constraints_still_apply_to_uv_sources() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &["fork>=2"]);
    approve(root.path(), "fork");
    let manifest = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        format!("{manifest}\n[tool.uv.sources]\nfork = {{ git = '{url}', rev = '{commit}' }}"),
    )
    .unwrap();
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "Python dependency resolution failed",
    );
}

#[tokio::test]
async fn git_sources_are_inherited_from_a_workspace_root() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    approve(root.path(), "fork");
    fs::write(root.path().join("pyproject.toml"), format!("[tool.uv.workspace]\nmembers=['packages/*']\n[tool.uv.sources]\nfork = {{ git = '{url}', rev = '{commit}' }}\n")).unwrap();
    python_project(&root.path().join("packages/app"), "app", "dependencies=['fork']");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(&root.path().join("packages/app"))
        .args(["-c", "import fork"])
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
}

#[tokio::test]
async fn git_projects_without_a_pyproject_use_the_legacy_backend() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (url, _) = repository(repo.path());
    fs::remove_file(repo.path().join("pyproject.toml")).unwrap();
    fs::rename(repo.path().join("src/fork"), repo.path().join("fork")).unwrap();
    fs::write(repo.path().join("setup.py"), "# legacy Python project\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "legacy",
        ],
    );
    let commit = git(repo.path(), &["rev-parse", "HEAD"]);
    let mut server = mockito::Server::new_async().await;
    let backend = super::TINY_BACKEND
        .replace(
            r#"manifest = tomllib.load(open("pyproject.toml", "rb"))"#,
            "manifest = {'project': {'name': 'fork', 'version': '1.0'}}",
        )
        .replace(r#"return "src" if os.path.isdir("src") else ".""#, r#"return ".""#);
    let backend = format!(
        "{backend}\nclass Legacy:\n    build_wheel = staticmethod(build_wheel)\n__legacy__ = Legacy()\n",
    );
    let _setuptools = serve(
        &mut server,
        "setuptools",
        &[("80.0", wheel("setuptools", "80.0", "", &[("setuptools/build_meta.py", &backend)]))],
    )
    .await;
    let _wheel = serve(&mut server, "wheel", &[("1.0", wheel("wheel", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[&format!("fork @ git+{url}@{commit}")]);
    approve(root.path(), "fork");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import fork"])
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
}

#[tokio::test]
async fn nested_git_builds_reuse_completed_backend_environments() {
    let root = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (first_url, first_commit) = repository(first.path());
    let (second_url, _) = repository(second.path());
    python_project(second.path(), "otherfork", "dependencies = []");
    git(second.path(), &["add", "."]);
    git(
        second.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "rename",
        ],
    );
    let second_commit = git(second.path(), &["rev-parse", "HEAD"]);
    let mut server = mockito::Server::new_async().await;
    let mut backends = serve_backends(&mut server).await;
    let index = backends.pop().unwrap().expect(2);
    project(root.path(), &server.url(), &[]);
    approve(root.path(), "fork");
    approve(root.path(), "otherfork");
    python_project(root.path(), "app", "dependencies = []");
    let manifest = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
    fs::write(root.path().join("pyproject.toml"), manifest.replace(
        "requires = ['tinybackend']",
        &format!("requires = ['tinybackend', 'fork @ git+{first_url}@{first_commit}', 'otherfork @ git+{second_url}@{second_commit}']"),
    )).unwrap();
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import app"])
        .assert()
        .success();
    index.assert_async().await;
}

#[tokio::test]
async fn cyclic_git_build_requirements_fail_without_hanging() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (url, _) = repository(repo.path());
    let manifest = fs::read_to_string(repo.path().join("pyproject.toml"))
        .unwrap()
        .replace("requires = ['tinybackend']", &format!("requires = ['fork @ git+{url}@main']"));
    fs::write(repo.path().join("pyproject.toml"), manifest).unwrap();
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "cycle"],
    );
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &[&format!("fork @ git+{url}@main")]);
    approve(root.path(), "fork");
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "cyclic Python build requirements",
    );
}

#[tokio::test]
async fn git_submodules_are_preserved_in_offline_checkouts() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let child = tempfile::tempdir().unwrap();
    let (child_url, _) = repository(child.path());
    let (url, _) = repository(repo.path());
    git(repo.path(), &["rm", "-r", "src/fork"]);
    git(
        repo.path(),
        &["-c", "protocol.file.allow=always", "submodule", "add", &child_url, "src/fork"],
    );
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "submodule",
        ],
    );
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[&format!("fork @ git+{url}@main#subdirectory=src/fork")]);
    approve(root.path(), "fork");
    pacquet_in(root.path())
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_0", "always")
        .arg("install")
        .assert()
        .success();
    drop(child);
    drop(repo);
    pacquet_in(root.path())
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_0", "always")
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import fork"])
        .assert()
        .success();
}

fn declare_platforms(root: &Path) {
    let mut platforms = String::new();
    for (platform, _) in super::declared_platforms() {
        writeln!(platforms, "    - {platform}").unwrap();
    }
    super::add_python_settings(root, &format!("  platforms:\n{platforms}"));
}

#[tokio::test]
async fn remote_and_index_sources_of_one_version_replay_for_the_correct_platform() {
    let repo = tempfile::tempdir().unwrap();
    let (url, commit) = repository(repo.path());
    for vcs in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _backends = serve_backends(&mut server).await;
        let _fork = serve(
            &mut server,
            "fork",
            &[("1.0", wheel("fork", "1.0", "Requires-Dist: helper\n", &[]))],
        )
        .await;
        let _helper =
            serve(&mut server, "helper", &[("1.0", wheel("helper", "1.0", "", &[]))]).await;
        let archive = wheel("fork", "1.0", "", &[("fork/origin.py", "MARKER='direct fork'\n")]);
        let digest = format!("{:x}", Sha256::digest(&archive));
        let _artifact = server
            .mock("GET", "/direct/fork-1.0-py3-none-any.whl")
            .with_body(archive)
            .expect_at_least(0)
            .create_async()
            .await;
        let source = if vcs {
            format!("git+{url}@{commit}")
        } else {
            format!("{}/direct/fork-1.0-py3-none-any.whl#sha256={digest}", server.url())
        };
        let platform = match std::env::consts::OS {
            "windows" => "win32",
            "macos" => "darwin",
            other => other,
        };
        project(
            root.path(),
            &server.url(),
            &[
                &format!("fork @ {source} ; sys_platform == '{platform}'"),
                &format!("fork ; sys_platform != '{platform}'"),
            ],
        );
        approve(root.path(), "fork");
        declare_platforms(root.path());
        pacquet_in(root.path())
            .arg("install")
            .assert()
            .success();
        let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        let parsed: toml::Value = toml::from_str(&lock).unwrap();
        assert_eq!(
            parsed["packages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|package| package["name"].as_str() == Some("fork"))
                .count(),
            2,
        );
        let code = if vcs {
            "import fork; assert fork.MARKER == 'workspace fork'"
        } else {
            "import fork.origin; assert fork.origin.MARKER == 'direct fork'"
        };
        let code = format!(
            "{code}; import importlib.util; assert importlib.util.find_spec('helper') is None",
        );
        python(root.path())
            .args(["-c", &code])
            .assert()
            .success();
        pacquet_in(root.path())
            .args(["install", "--offline", "--frozen-lockfile"])
            .assert()
            .success();
        python(root.path())
            .args(["-c", &code])
            .assert()
            .success();
    }
}

#[tokio::test]
async fn different_git_versions_keep_their_built_files_until_installation() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (url, first) = repository(repo.path());
    let manifest = fs::read_to_string(repo.path().join("pyproject.toml"))
        .unwrap()
        .replace("version = '1.0'", "version = '2.0'");
    fs::write(repo.path().join("pyproject.toml"), manifest).unwrap();
    git(repo.path(), &["add", "."]);
    git(
        repo.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "second version",
        ],
    );
    let second = git(repo.path(), &["rev-parse", "HEAD"]);
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    project(
        root.path(),
        &server.url(),
        &[
            &format!("fork @ git+{url}@{first} ; sys_platform == '{platform}'"),
            &format!("fork @ git+{url}@{second} ; sys_platform != '{platform}'"),
        ],
    );
    approve(root.path(), "fork");
    declare_platforms(root.path());
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let code = "import fork, importlib.metadata as m; assert m.version('fork') == '1.0'";
    python(root.path())
        .args(["-c", code])
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", code])
        .assert()
        .success();
}

#[tokio::test]
async fn narrowed_remote_sources_are_rejected_before_fetching() {
    for kind in ["url", "git"] {
        for (narrowing, diagnostic) in [
            (r#"marker = "sys_platform == 'win32'""#, "conditional"),
            ("extra = 'test'", "extra-qualified"),
            ("group = 'test'", "group-qualified"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let server = mockito::Server::new_async().await;
            project(root.path(), &server.url(), &["alpha>=1"]);
            let path = root.path().join("pyproject.toml");
            let manifest = fs::read_to_string(&path).unwrap();
            fs::write(&path, format!("{manifest}\n[tool.uv.sources]\nalpha = {{ {kind} = 'https://example.test/alpha', {narrowing} }}\n")).unwrap();
            assert_failure_contains(
                pacquet_in(root.path()).arg("install"),
                &format!("pnpm does not support the {diagnostic} Python source"),
            );
        }
    }
}

#[tokio::test]
async fn offline_git_cache_errors_redact_ssh_userinfo() {
    let root = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &["alpha @ git+ssh://secret-token@example.test/repo@main"]);
    approve(root.path(), "alpha");
    let output = pacquet_in(root.path())
        .args(["install", "--offline"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not cached for offline"), "{stderr}");
    assert!(!stderr.contains("secret-token"), "{stderr}");
}
