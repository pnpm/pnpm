use super::_utils::pacquet_in;
use assert_cmd::prelude::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs,
    io::{Cursor, Write},
    path::Path,
    process::Command,
};
use zip::{ZipWriter, write::SimpleFileOptions};

fn wheel(name: &str, version: &str, metadata: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let dist_info = format!("{name}-{version}.dist-info");
    let mut files = vec![
        (
            format!("{name}/__init__.py"),
            format!("VERSION = '{version}'\ndef main():\n    print(VERSION)\n"),
        ),
        (
            format!("{dist_info}/METADATA"),
            format!("Metadata-Version: 2.4\nName: {name}\nVersion: {version}\n{metadata}\n"),
        ),
        (
            format!("{dist_info}/WHEEL"),
            "Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n".to_string(),
        ),
    ];
    files.extend(extra.iter().map(|(path, contents)| (path.to_string(), contents.to_string())));
    let mut record = String::new();
    for (path, contents) in &files {
        writeln!(
            record,
            "{path},sha256={},{}",
            URL_SAFE_NO_PAD.encode(Sha256::digest(contents)),
            contents.len(),
        )
        .unwrap();
    }
    writeln!(record, "{dist_info}/RECORD,,").unwrap();
    files.push((format!("{dist_info}/RECORD"), record));
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in files {
        archive.start_file(path, SimpleFileOptions::default()).unwrap();
        archive.write_all(contents.as_bytes()).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

async fn serve(
    server: &mut mockito::ServerGuard,
    name: &str,
    versions: &[(&str, Vec<u8>)],
) -> Vec<mockito::Mock> {
    let mut mocks = Vec::new();
    let mut files = Vec::new();
    for (version, archive) in versions {
        let filename = format!("{name}-{version}-py3-none-any.whl");
        files.push(json!({"filename": filename, "url": format!("/files/{filename}"), "hashes": {"sha256": format!("{:x}", Sha256::digest(archive))}}));
        mocks.push(
            server
                .mock("GET", format!("/files/{filename}").as_str())
                .with_body(archive)
                .expect_at_least(0)
                .create_async()
                .await,
        );
    }
    mocks.push(
        server
            .mock("GET", format!("/simple/{name}/").as_str())
            .match_header("accept", "application/vnd.pypi.simple.v1+json")
            .with_header("content-type", "application/vnd.pypi.simple.v1+json")
            .with_body(
                json!({"meta": {"api-version": "1.0"}, "name": name, "files": files}).to_string(),
            )
            .expect_at_least(1)
            .create_async()
            .await,
    );
    mocks
}

fn project(root: &Path, index: &str, dependencies: &[&str]) {
    fs::write(root.join("pnpm-workspace.yaml"), format!("python:\n  enabled: true\n  indexUrl: '{index}/simple/'\nstoreDir: '{}'\ncacheDir: '{}'\nfetchRetries: 0\n", root.join("store").display(), root.join("cache").display())).unwrap();
    fs::write(root.join("pyproject.toml"), format!("[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\ndependencies = {dependencies:?}\n")).unwrap();
}

fn python(root: &Path) -> Command {
    Command::new(root.join(if cfg!(windows) {
        ".venv/Scripts/python.exe"
    } else {
        ".venv/bin/python"
    }))
}

#[tokio::test]
async fn discovers_independent_python_projects_and_ignores_environment_manifests() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::write(root.path().join("pyproject.toml"), "[tool.ruff]\nline-length = 100\n").unwrap();
    for directory in ["app-one", "app-two", ".venv/ignored", ".pnpm/ignored"] {
        let path = root.path().join(directory);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("pyproject.toml"),
            "[project]\nname = 'app'\nversion = '1.0'\ndependencies = ['alpha>=1']\n",
        )
        .unwrap();
    }
    pacquet_in(root.path()).arg("install").assert().success();
    for directory in ["app-one", "app-two"] {
        python(&root.path().join(directory)).args(["-c", "import alpha"]).assert().success();
        assert!(root.path().join(directory).join("pylock.toml").exists());
    }
    for directory in [".venv/ignored", ".pnpm/ignored"] {
        assert!(!root.path().join(directory).join("pylock.toml").exists());
    }
    assert!(!root.path().join("pylock.toml").exists());
}

#[tokio::test]
async fn rejects_archive_integrity_failure_and_offline_store_misses() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let index = server.mock("GET", "/simple/alpha/").with_body(json!({"files": [{"filename": "alpha-1.0-py3-none-any.whl", "url": "/wheel", "hashes": {"sha256": "0".repeat(64)}}]}).to_string()).create_async().await;
    let artifact =
        server.mock("GET", "/wheel").with_body(wheel("alpha", "1.0", "", &[])).create_async().await;
    project(root.path(), &server.url(), &["alpha"]);
    pacquet_in(root.path()).arg("install").assert().failure();
    index.assert_async().await;
    artifact.assert_async().await;
    assert!(!root.path().join(".venv").exists());
    drop(server);
    pacquet_in(root.path()).args(["install", "--offline"]).assert().failure();
    assert!(!root.path().join("pylock.toml").exists());
}

#[tokio::test]
async fn rejects_conflicts_and_cyclic_dependency_groups_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "", &[])), ("2.0", wheel("alpha", "2.0", "", &[]))],
    )
    .await;
    project(root.path(), &server.url(), &["alpha<2", "alpha>=2"]);
    pacquet_in(root.path()).arg("install").assert().failure();
    assert!(!root.path().join("pylock.toml").exists());
    fs::write(root.path().join("pyproject.toml"), "[project]\nname = 'app'\nversion = '1.0'\n[dependency-groups]\ndev = [{include-group = 'test'}]\ntest = [{include-group = 'dev'}]\n").unwrap();
    pacquet_in(root.path()).arg("install").assert().failure();
    assert!(!root.path().join(".venv").exists());
}

#[tokio::test]
async fn dependency_cycles_resolve_and_tampered_lockfile_closure_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta>=1", &[]))],
    )
    .await;
    let _beta = serve(
        &mut server,
        "beta",
        &[("1.0", wheel("beta", "1.0", "Requires-Dist: alpha>=1", &[]))],
    )
    .await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path()).arg("install").assert().success();
    let environment = pnpm_fs::read_symlink_dir(&root.path().join(".venv")).unwrap();
    let mut lock: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pylock.toml")).unwrap()).unwrap();
    lock["packages"]
        .as_array_mut()
        .unwrap()
        .retain(|package| package["name"].as_str() != Some("beta"));
    fs::write(root.path().join("pylock.toml"), toml::to_string(&lock).unwrap()).unwrap();
    pacquet_in(root.path()).args(["install", "--offline", "--frozen-lockfile"]).assert().failure();
    assert_eq!(environment, pnpm_fs::read_symlink_dir(&root.path().join(".venv")).unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_symlinked_generation_parent_without_writing_outside_the_project() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &[]);
    std::os::unix::fs::symlink(outside.path(), root.path().join(".pnpm")).unwrap();
    pacquet_in(root.path()).arg("install").assert().failure();
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert!(!root.path().join("pylock.toml").exists());
}

#[tokio::test]
async fn installs_real_environment_with_ranges_extras_markers_scripts_and_offline_replay() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "Provides-Extra: speed\nRequires-Dist: beta>=1,<2; extra == 'speed'\nRequires-Dist: unavailable; python_version < '2'", &[
        ("alpha-1.0.dist-info/entry_points.txt", "[console_scripts]\nalpha-cli = alpha:main\n"),
        ("alpha-1.0.data/data/share/alpha.txt", "data file"),
    ]))]).await;
    let beta = serve(
        &mut server,
        "beta",
        &[("1.0", wheel("beta", "1.0", "", &[])), ("2.0", wheel("beta", "2.0", "", &[]))],
    )
    .await;
    project(root.path(), &server.url(), &["alpha[speed]>=1"]);
    pacquet_in(root.path()).arg("install").assert().success();
    python(root.path())
        .args(["-c", "import alpha, beta; assert beta.VERSION == '1.0'"])
        .assert()
        .success();
    let command = root.path().join(if cfg!(windows) {
        ".venv/Scripts/alpha-cli.cmd"
    } else {
        ".venv/bin/alpha-cli"
    });
    Command::new(command).assert().success().stdout("1.0\n");
    assert_eq!(fs::read_to_string(root.path().join(".venv/share/alpha.txt")).unwrap(), "data file");
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&lock).unwrap();
    assert_eq!(parsed["lock-version"].as_str(), Some("1.0"));
    assert_eq!(parsed["packages"].as_array().unwrap().len(), 2);
    assert!(!root.path().join("pnpm-lock.yaml").exists());
    assert!(!root.path().join("package.json").exists());
    for mock in alpha.into_iter().chain(beta) {
        mock.assert_async().await;
    }
    drop(server);
    pnpm_fs::remove_symlink_dir(&root.path().join(".venv")).unwrap();
    pacquet_in(root.path()).args(["install", "--offline", "--frozen-lockfile"]).assert().success();
    python(root.path()).args(["-c", "import alpha, beta"]).assert().success();
    pacquet_in(root.path()).args(["exec", "python", "-c", "import alpha, beta"]).assert().success();
    assert_eq!(lock, fs::read_to_string(root.path().join("pylock.toml")).unwrap());
}

#[tokio::test]
async fn backtracks_instead_of_rejecting_conflicting_latest_versions() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(
        &mut server,
        "alpha",
        &[
            ("1.0", wheel("alpha", "1.0", "Requires-Dist: beta<2", &[])),
            ("2.0", wheel("alpha", "2.0", "Requires-Dist: beta>=2", &[])),
        ],
    )
    .await;
    let _beta = serve(
        &mut server,
        "beta",
        &[("1.0", wheel("beta", "1.0", "", &[])), ("2.0", wheel("beta", "2.0", "", &[]))],
    )
    .await;
    project(root.path(), &server.url(), &["alpha>=1", "beta<2"]);
    pacquet_in(root.path()).arg("install").assert().success();
    python(root.path())
        .args(["-c", "import alpha, beta; assert alpha.VERSION == beta.VERSION == '1.0'"])
        .assert()
        .success();
}

#[tokio::test]
async fn add_updates_pyproject_and_lockfile_without_creating_node_metadata() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    pacquet_in(root.path()).args(["add", "pypi:alpha@>=1", "--save-dev"]).assert().success();
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pyproject.toml")).unwrap()).unwrap();
    assert_eq!(manifest["dependency-groups"]["dev"][0].as_str(), Some("alpha>=1"));
    assert!(!root.path().join("package.json").exists());
    assert!(!root.path().join("Cargo.toml").exists());
    python(root.path()).args(["-c", "import alpha"]).assert().success();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile", "--prod"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import importlib.util; assert importlib.util.find_spec('alpha') is None"])
        .assert()
        .success();
}

#[tokio::test]
async fn frozen_lockfile_rejects_changed_manifest_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path()).args(["install", "--lockfile-only"]).assert().success();
    assert!(!root.path().join(".venv").exists());
    let lock = fs::read(root.path().join("pylock.toml")).unwrap();
    project(root.path(), &server.url(), &["alpha>=2"]);
    pacquet_in(root.path()).args(["install", "--frozen-lockfile"]).assert().failure();
    assert_eq!(fs::read(root.path().join("pylock.toml")).unwrap(), lock);
    assert!(!root.path().join(".venv").exists());
}

#[tokio::test]
async fn failed_mixed_add_restores_manifests_and_keeps_the_previous_environment() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    pacquet_in(root.path()).args(["install"]).assert().success();
    let previous_environment = pnpm_fs::read_symlink_dir(&root.path().join(".venv")).unwrap();
    let previous_lock = fs::read(root.path().join("pylock.toml")).unwrap();
    fs::write(root.path().join("package.json"), "{\"name\":\"app\",\"version\":\"1.0.0\"}\n")
        .unwrap();
    fs::write(root.path().join(".npmrc"), format!("registry={}\n", server.url())).unwrap();
    let manifest = fs::read(root.path().join("pyproject.toml")).unwrap();
    let node_manifest = fs::read(root.path().join("package.json")).unwrap();
    pacquet_in(root.path())
        .args(["add", "pypi:alpha@>=1", "nonexistent-node-package"])
        .assert()
        .failure();
    assert_eq!(manifest, fs::read(root.path().join("pyproject.toml")).unwrap());
    assert_eq!(node_manifest, fs::read(root.path().join("package.json")).unwrap());
    assert_eq!(previous_lock, fs::read(root.path().join("pylock.toml")).unwrap());
    assert_eq!(
        previous_environment,
        pnpm_fs::read_symlink_dir(&root.path().join(".venv")).unwrap(),
    );
}

#[tokio::test]
async fn add_pins_bare_requirements_and_preserves_unrelated_manifest_text() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::write(root.path().join("pyproject.toml"), "# keep this comment\n[project]\nname = 'app' # original quoting\nversion = '1.0'\n\n[tool.example]\nsetting = 'preserve'\n").unwrap();
    pacquet_in(root.path()).args(["add", "pypi:alpha", "--save-exact"]).assert().success();
    let text = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
    assert!(text.starts_with("# keep this comment\n[project]"));
    assert!(text.contains("name = 'app' # original quoting"));
    assert!(text.contains("[tool.example]\nsetting = 'preserve'"));
    assert!(text.contains("alpha==1.0"));
    pacquet_in(root.path()).args(["install", "--offline", "--frozen-lockfile"]).assert().success();
}

#[tokio::test]
async fn installs_node_cargo_and_python_through_the_real_coordinator() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!("{workspace}\ncargo:\n  enabled: true\n"),
    )
    .unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = 'mixed-app'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::create_dir(root.path().join("node-package")).unwrap();
    fs::write(
        root.path().join("node-package/package.json"),
        r#"{"name":"local-node","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(root.path().join("package.json"), r#"{"name":"mixed-app","version":"1.0.0","dependencies":{"local-node":"link:./node-package"},"scripts":{"python-check":"python -c \"import alpha\""}}"#).unwrap();
    pacquet_in(root.path()).arg("install").assert().success();
    assert!(root.path().join("node_modules/local-node/package.json").exists());
    assert!(root.path().join("pnpm-lock.yaml").exists());
    assert!(root.path().join("Cargo.lock").exists());
    assert!(root.path().join("pylock.toml").exists());
    Command::new("cargo")
        .current_dir(root.path())
        .args(["check", "--offline", "--locked"])
        .assert()
        .success();
    pacquet_in(root.path()).args(["run", "python-check"]).assert().success();
}

#[tokio::test]
async fn disabled_python_and_tool_only_pyprojects_do_not_probe_an_interpreter() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "python:\n  executable: this-interpreter-does-not-exist\n",
    )
    .unwrap();
    fs::write(root.path().join("pyproject.toml"), "this is not TOML").unwrap();
    fs::write(root.path().join("package.json"), "{}").unwrap();
    pacquet_in(root.path()).arg("install").assert().success();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "python:\n  enabled: true\n  executable: this-interpreter-does-not-exist\n",
    )
    .unwrap();
    fs::write(root.path().join("pyproject.toml"), "[tool.ruff]\nline-length = 100\n").unwrap();
    pacquet_in(root.path()).arg("install").assert().success();
    assert!(!root.path().join("pylock.toml").exists());
}

#[tokio::test]
async fn rejects_corrupt_record_and_leaves_no_environment_or_lockfile() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let archive = wheel("alpha", "1.0", "", &[]);
    let mut archive = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let mut altered = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        altered.start_file(entry.name(), SimpleFileOptions::default()).unwrap();
        if entry.name() == "alpha/__init__.py" {
            altered.write_all(b"TAMPERED = True\n").unwrap();
        } else {
            std::io::copy(&mut entry, &mut altered).unwrap();
        }
    }
    let _alpha =
        serve(&mut server, "alpha", &[("1.0", altered.finish().unwrap().into_inner())]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path()).arg("install").assert().failure();
    assert!(!root.path().join("pylock.toml").exists());
    assert!(!root.path().join(".venv").exists());
}

#[tokio::test]
async fn refuses_unmanaged_environment_and_rolls_back_add() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::create_dir(root.path().join(".venv")).unwrap();
    fs::write(root.path().join(".venv/owned-by-user"), "preserve").unwrap();
    let manifest = fs::read(root.path().join("pyproject.toml")).unwrap();
    pacquet_in(root.path()).args(["add", "pypi:alpha"]).assert().failure();
    assert_eq!(manifest, fs::read(root.path().join("pyproject.toml")).unwrap());
    assert_eq!(fs::read_to_string(root.path().join(".venv/owned-by-user")).unwrap(), "preserve");
}
