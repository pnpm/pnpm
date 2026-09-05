use super::_utils::{flatten_report, pacquet_in};
use assert_cmd::prelude::*;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs,
    io::{Cursor, Write},
    path::Path,
    process::Command,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use zip::{ZipWriter, write::SimpleFileOptions};

fn wheel(name: &str, version: &str, metadata: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    wheel_with_tags(name, version, metadata, extra, "Tag: py3-none-any\n")
}

fn wheel_with_tags(
    name: &str,
    version: &str,
    metadata: &str,
    extra: &[(&str, &str)],
    tags: &str,
) -> Vec<u8> {
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
            format!("Wheel-Version: 1.0\nRoot-Is-Purelib: true\n{tags}"),
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
    serve_with_index_auth(server, name, versions, None).await
}

async fn serve_with_index_auth(
    server: &mut mockito::ServerGuard,
    name: &str,
    versions: &[(&str, Vec<u8>)],
    authorization: Option<&str>,
) -> Vec<mockito::Mock> {
    let mut mocks = Vec::new();
    let mut files = Vec::new();
    for (version, archive) in versions {
        let filename = format!("{name}-{version}-py3-none-any.whl");
        files.push(json!({"filename": filename, "url": format!("/files/{filename}"), "hashes": {"sha256": format!("{:x}", Sha256::digest(archive))}}));
        mocks.push(
            server
                .mock("GET", format!("/files/{filename}").as_str())
                .match_header("authorization", mockito::Matcher::Missing)
                .with_body(archive)
                .expect_at_least(0)
                .create_async()
                .await,
        );
    }
    mocks.push(
        server
            .mock("GET", format!("/simple/{name}/").as_str())
            .match_header(
                "authorization",
                authorization.map_or(mockito::Matcher::Missing, |value| {
                    mockito::Matcher::Exact(value.to_string())
                }),
            )
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

fn assert_failure_contains(command: &mut Command, expected: &str) {
    let result = command.assert().failure();
    let stderr = String::from_utf8_lossy(&result.get_output().stderr);
    eprintln!("stderr:\n{stderr}");
    assert!(flatten_report(&stderr).contains(&flatten_report(expected)));
}

fn cargo_project(root: &Path, name: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nname = '{name}'\nversion = '0.1.0'\nedition = '2024'\n"),
    )
    .unwrap();
}

#[test]
fn repeated_install_excludes_configured_stores_and_caches_from_native_discovery() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "https://unused.invalid", &[]);
    cargo_project(root.path(), "app");
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!("{workspace}\ncargo:\n  enabled: true\n"),
    )
    .unwrap();
    pacquet_in(root.path()).args(["install", "--offline"]).assert().success();
    for relative in ["store/v11/crates/cached", "cache/unpacked-project"] {
        let directory = root.path().join(relative);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("Cargo.toml"), "this is cached data, not a workspace manifest")
            .unwrap();
        fs::write(
            directory.join("pyproject.toml"),
            "this is cached data, not a workspace manifest",
        )
        .unwrap();
    }
    pacquet_in(root.path()).args(["install", "--offline", "--frozen-lockfile"]).assert().success();
}

#[test]
fn failed_python_preparation_does_not_publish_cargo_metadata() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "https://unused.invalid", &[]);
    cargo_project(root.path(), "app");
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!("{workspace}\ncargo:\n  enabled: true\n"),
    )
    .unwrap();
    fs::write(root.path().join("pyproject.toml"), "not valid TOML").unwrap();
    pacquet_in(root.path()).args(["install", "--offline"]).assert().failure();
    for relative in ["Cargo.lock", ".cargo/config.toml", "pylock.toml", ".venv"] {
        let path = root.path().join(relative);
        assert!(!path.exists(), "failed preparation must not publish {path:?}");
    }
}

#[test]
fn failed_publication_restores_prior_cargo_workspaces_and_discards_python_generation() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "https://unused.invalid", &[]);
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!("{workspace}\ncargo:\n  enabled: true\n"),
    )
    .unwrap();
    for name in ["rust-a", "rust-b"] {
        let directory = root.path().join(name);
        cargo_project(&directory, name);
        fs::create_dir(directory.join(".cargo")).unwrap();
    }
    let first_config = root.path().join("rust-a/.cargo/config.toml");
    let second_config = root.path().join("rust-b/.cargo/config.toml");
    fs::write(&first_config, "# preserve user settings\n").unwrap();
    fs::write(&second_config, "# >>> pnpm-managed cargo sources >>>\n").unwrap();
    let output = pacquet_in(root.path()).args(["install", "--offline"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("{stderr}");
    assert!(!output.status.success());
    assert!(stderr.contains("incomplete pnpm-managed Cargo source block"));
    assert_eq!(fs::read_to_string(first_config).unwrap(), "# preserve user settings\n");
    assert_eq!(
        fs::read_to_string(second_config).unwrap(),
        "# >>> pnpm-managed cargo sources >>>\n",
    );
    for relative in ["rust-a/Cargo.lock", "rust-b/Cargo.lock", "pylock.toml", ".venv"] {
        let path = root.path().join(relative);
        assert!(!path.exists(), "failed publication must restore {path:?}");
    }
    assert_eq!(fs::read_dir(root.path().join(".pnpm/python-envs")).unwrap().count(), 0);
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
    Command::new(command).assert().success().stdout(if cfg!(windows) {
        "1.0\r\n"
    } else {
        "1.0\n"
    });
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
    let replayed_lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("INITIAL LOCK:\n{lock}\nREPLAYED LOCK:\n{replayed_lock}");
    assert_eq!(lock, replayed_lock);
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
async fn rejects_wheel_tags_that_disagree_with_the_filename() {
    for tags in ["", "Tag: py2-none-any\n", "Tag: py3-none-any\nTag: cp311-cp311-win_amd64\n"] {
        eprintln!("WHEEL tags: {tags:?}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _alpha =
            serve(&mut server, "alpha", &[("1.0", wheel_with_tags("alpha", "1.0", "", &[], tags))])
                .await;
        project(root.path(), &server.url(), &["alpha"]);
        assert_failure_contains(
            pacquet_in(root.path()).arg("install"),
            "wheel Tag fields do not match filename",
        );
        assert!(!root.path().join("pylock.toml").exists(), "published rejected wheel lockfile");
        assert!(!root.path().join(".venv").exists(), "published rejected wheel environment");
    }
}

#[tokio::test]
async fn python_index_and_wheel_requests_do_not_inherit_npm_credentials() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let leaked = server
        .mock("GET", mockito::Matcher::Any)
        .match_header("authorization", "Bearer victim-npm-token")
        .with_status(403)
        .expect(0)
        .create_async()
        .await;
    project(root.path(), &server.url(), &["alpha"]);
    pacquet_in(root.path())
        .env(
            format!("npm_config_{}:_authToken", pnpm_network::nerf_dart(&server.url())),
            "victim-npm-token",
        )
        .arg("install")
        .assert()
        .success();
    leaked.assert_async().await;
    python(root.path()).args(["-c", "import alpha"]).assert().success();
}

#[tokio::test]
async fn python_index_uses_only_its_explicit_credentials() {
    for (username, password) in
        [("user", "secret"), ("user&name", "secret&suffix+space"), ("usér", "sëcret")]
    {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let authorization = format!("Basic {}", STANDARD.encode(format!("{username}:{password}")));
        let _alpha = serve_with_index_auth(
            &mut server,
            "alpha",
            &[("1.0", wheel("alpha", "1.0", "", &[]))],
            Some(&authorization),
        )
        .await;
        let mut index: url::Url = server.url().parse().unwrap();
        index.set_username(username).unwrap();
        index.set_password(Some(password)).unwrap();
        project(root.path(), index.as_str().trim_end_matches('/'), &["alpha"]);
        let workspace_path = root.path().join("pnpm-workspace.yaml");
        let workspace = fs::read_to_string(&workspace_path).unwrap();
        fs::write(workspace_path, workspace.replace("/simple/'", "/simple'")).unwrap();
        pacquet_in(root.path()).arg("install").assert().success();
        let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        eprintln!("lockfile:\n{lock}");
        assert!(!lock.contains(password), "credentials leaked into lockfile");
        python(root.path()).args(["-c", "import alpha"]).assert().success();
    }
}

#[tokio::test]
async fn accepts_expanded_internal_tags_for_a_compressed_wheel_filename() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let archive =
        wheel_with_tags("alpha", "1.0", "", &[], "Tag: py2-none-any\nTag: py3-none-any\n");
    let filename = "alpha-1.0-py2.py3-none-any.whl";
    let metadata = json!({"files": [{
        "filename": filename,
        "url": format!("/files/{filename}"),
        "hashes": {"sha256": format!("{:x}", Sha256::digest(&archive))},
    }]});
    let index = server
        .mock("GET", "/simple/alpha/")
        .with_body(metadata.to_string())
        .expect(1)
        .create_async()
        .await;
    let download = server
        .mock("GET", format!("/files/{filename}").as_str())
        .with_body(archive)
        .expect(1)
        .create_async()
        .await;
    project(root.path(), &server.url(), &["alpha"]);
    pacquet_in(root.path()).arg("install").assert().success();
    python(root.path()).args(["-c", "import alpha"]).assert().success();
    index.assert_async().await;
    download.assert_async().await;
}

#[tokio::test]
async fn rejects_oversized_python_index_without_caching_or_publication() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/simple/alpha/")
        .with_chunked_body(|writer| {
            for _ in 0..8193 {
                writer.write_all(&[b' '; 8192])?;
            }
            Ok(())
        })
        .expect(1)
        .create_async()
        .await;
    project(root.path(), &server.url(), &["alpha"]);
    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "Python index response for alpha exceeds",
    );
    request.assert_async().await;
    assert!(!root.path().join("cache/python-index-v2").exists(), "cached oversized response");
    assert!(!root.path().join("pylock.toml").exists(), "published oversized response lockfile");
}

#[test]
fn rejects_oversized_python_index_cache_before_parsing() {
    let root = tempfile::tempdir().unwrap();
    let index = "https://unused.invalid";
    project(root.path(), index, &["alpha"]);
    let cache = root.path().join("cache/python-index-v2");
    fs::create_dir_all(&cache).unwrap();
    fs::File::create(cache.join(format!(
        "{}.json",
        pnpm_crypto_hash::create_hex_hash(&format!("{index}/simple/alpha/")),
    )))
    .unwrap()
    .set_len(64 * 1024 * 1024 + 64 * 1024 + 1)
    .unwrap();
    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--offline"]),
        "Python index cache for alpha exceeds",
    );
}

#[tokio::test]
async fn caches_python_index_as_raw_json_and_reuses_it_offline() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha"]);
    pacquet_in(root.path()).arg("install").assert().success();
    let cache = fs::read_dir(root.path().join("cache/python-index-v2"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let cached: serde_json::Value = serde_json::from_slice(&fs::read(cache).unwrap()).unwrap();
    dbg!(&cached);
    assert!(cached["body"]["files"].is_array(), "metadata was not stored as a JSON object");
    pacquet_in(root.path()).args(["add", "pypi:alpha", "--offline"]).assert().success();
}

#[test]
fn broken_python_environment_errors_identify_the_missing_path() {
    for missing_target in [false, true] {
        eprintln!("missing_target={missing_target}");
        let root = tempfile::tempdir().unwrap();
        project(root.path(), "https://unused.invalid", &[]);
        let target = if missing_target {
            root.path().join(".pnpm/python-envs/env-missing")
        } else {
            root.path().join("unmanaged")
        };
        fs::create_dir_all(&target).unwrap();
        pnpm_fs::force_symlink_dir(&target, &root.path().join(".venv")).unwrap();
        if missing_target {
            fs::remove_dir(&target).unwrap();
        }
        let missing = if missing_target { target } else { root.path().join(".pnpm/python-envs") };
        assert_failure_contains(
            pacquet_in(root.path()).args(["install", "--offline"]),
            &format!("{} for {}", missing.display(), root.path().join(".venv").display()),
        );
        assert!(!root.path().join("pylock.toml").exists(), "published failed environment metadata");
    }
}

#[tokio::test]
async fn frozen_wheel_downloads_overlap_and_settle_before_reporting_failure() {
    for fail in [false, true] {
        eprintln!("fail={fail}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let archives =
            [("alpha", wheel("alpha", "1.0", "", &[])), ("beta", wheel("beta", "1.0", "", &[]))];
        let mut initial_requests = Vec::new();
        for (name, archive) in &archives {
            initial_requests.extend(serve(&mut server, name, &[("1.0", archive.clone())]).await);
        }
        project(root.path(), &server.url(), &["alpha", "beta"]);
        pacquet_in(root.path()).args(["install", "--lockfile-only"]).assert().success();
        for request in initial_requests {
            request.remove_async().await;
        }
        let before = fs::read(root.path().join("pylock.toml")).unwrap();
        let rendezvous = Arc::new((Mutex::new(0), Condvar::new()));
        let sibling_finished = Arc::new(AtomicBool::new(false));
        let mut downloads = Vec::new();
        for (name, archive) in archives {
            let rendezvous = Arc::clone(&rendezvous);
            let sibling_finished = Arc::clone(&sibling_finished);
            downloads.push(
                server
                    .mock("GET", format!("/files/{name}-1.0-py3-none-any.whl").as_str())
                    .with_chunked_body(move |writer| {
                        let (arrivals, wake) = &*rendezvous;
                        let mut arrivals = arrivals.lock().unwrap();
                        *arrivals += 1;
                        wake.notify_all();
                        let (arrivals, timeout) = wake
                            .wait_timeout_while(arrivals, Duration::from_secs(10), |arrivals| {
                                *arrivals < 2
                            })
                            .unwrap();
                        drop(arrivals);
                        if timeout.timed_out() {
                            return Err(std::io::Error::other("wheel fetches did not overlap"));
                        }
                        if fail && name == "alpha" {
                            return writer.write_all(b"corrupt wheel");
                        }
                        if fail {
                            std::thread::sleep(Duration::from_millis(250));
                        }
                        writer.write_all(&archive)?;
                        if name == "beta" {
                            sibling_finished.store(true, Ordering::SeqCst);
                        }
                        Ok(())
                    })
                    .expect(1)
                    .create_async()
                    .await,
            );
        }
        let mut command = pacquet_in(root.path());
        command
            .env("PNPM_CONFIG_STORE_DIR", root.path().join("cold-store"))
            .env("PNPM_CONFIG_NETWORK_CONCURRENCY", "2")
            .args(["install", "--frozen-lockfile"]);
        if fail {
            command.assert().failure();
            assert!(!root.path().join(".venv").exists(), "published a failed environment");
        } else {
            command.assert().success();
            python(root.path()).args(["-c", "import alpha, beta"]).assert().success();
        }
        assert!(sibling_finished.load(Ordering::SeqCst), "returned before sibling body finished");
        assert_eq!(before, fs::read(root.path().join("pylock.toml")).unwrap());
        for request in downloads {
            request.assert_async().await;
        }
    }
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
async fn wheel_scripts_rewrite_placeholder_shebangs_and_record_the_installed_bytes() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let scripts = [
        ("alpha-1.0.data/scripts/lf", "#!python\nprint('hello')\n"),
        ("alpha-1.0.data/scripts/crlf", "#!python\r\nprint('hello')\r\n"),
        ("alpha-1.0.data/scripts/gui", "#!pythonw\r\nprint('hello')\r\n"),
        ("alpha-1.0.data/scripts/other", "#!/bin/sh\nprintf hello\n"),
    ];
    let _requests =
        serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &scripts))]).await;
    project(root.path(), &server.url(), &["alpha"]);
    pacquet_in(root.path()).arg("install").assert().success();
    python(root.path()).args(["-c", r#"
import base64, csv, hashlib, sys, sysconfig
from pathlib import Path
scripts = Path(sysconfig.get_path('scripts'))
interpreter = scripts.resolve() / Path(sys.executable).name
for name, body in [('lf', b"print('hello')\n"), ('crlf', b"print('hello')\r\n"), ('gui', b"print('hello')\r\n")]:
    contents = (scripts / name).read_bytes()
    assert contents == ('#!' + str(interpreter) + '\n').encode() + body, contents
assert (scripts / 'other').read_bytes() == b'#!/bin/sh\nprintf hello\n'
site = Path(sysconfig.get_path('purelib'))
with (site / 'alpha-1.0.dist-info/RECORD').open(newline='') as record:
    for path, digest, size in csv.reader(record):
        if not digest:
            continue
        contents = (site / path).read_bytes()
        assert digest == 'sha256=' + base64.urlsafe_b64encode(hashlib.sha256(contents).digest()).rstrip(b'=').decode(), path
        assert int(size) == len(contents), path
"#]).assert().success();
}

#[tokio::test]
async fn inconsistent_python_lockfile_reports_a_dependency_explanation() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _requests = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    pacquet_in(root.path()).arg("install").assert().success();
    let environment = pnpm_fs::read_symlink_dir(&root.path().join(".venv")).unwrap();
    project(root.path(), &server.url(), &["alpha>=2"]);
    let mut lock: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pylock.toml")).unwrap()).unwrap();
    lock["tool"]["pnpm"]["requirements"][0] = toml::Value::String("alpha>=2".to_string());
    fs::write(root.path().join("pylock.toml"), toml::to_string(&lock).unwrap()).unwrap();
    let result = pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&result.get_output().stderr);
    eprintln!("stderr:\n{stderr}");
    assert!(
        flatten_report(&stderr)
            .contains(&flatten_report("Python lockfile does not satisfy the project:")),
    );
    assert!(stderr.contains("Python project"));
    assert!(!stderr.contains("NoSolution("));
    assert_eq!(environment, pnpm_fs::read_symlink_dir(&root.path().join(".venv")).unwrap());
}

#[test]
fn python_add_reports_unsupported_and_conflicting_save_flags() {
    for (flags, expected) in [
        (vec!["--save-build"], "--save-build requires at least one crate: dependency"),
        (vec!["--save-optional"], "do not support --save-build, --save-optional or --save-peer"),
        (vec!["--save-peer"], "do not support --save-build, --save-optional or --save-peer"),
        (vec!["--save-prod", "--save-dev"], "do not support combining --save-prod and --save-dev"),
    ] {
        eprintln!("flags={flags:?}");
        let root = tempfile::tempdir().unwrap();
        project(root.path(), "https://unused.invalid", &[]);
        assert_failure_contains(
            pacquet_in(root.path()).args(["add", "pypi:alpha"]).args(flags),
            expected,
        );
    }
}

#[tokio::test]
async fn add_supports_empty_and_populated_inline_python_tables() {
    for (manifest, development) in [
        ("project = {}\n", false),
        ("project = { name = 'app' }\n", false),
        ("project = {}\ndependency-groups = {}\n", true),
        ("project = {}\ndependency-groups = { test = [] }\n", true),
    ] {
        eprintln!("manifest={manifest:?}, development={development}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _requests =
            serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
        project(root.path(), &server.url(), &[]);
        fs::write(root.path().join("pyproject.toml"), manifest).unwrap();
        let mut command = pacquet_in(root.path());
        command.args(["add", "pypi:alpha", "--save-exact"]);
        if development {
            command.arg("--save-dev");
        }
        command.assert().success();
        let updated = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
        eprintln!("updated={updated}");
        let parsed: toml::Value = toml::from_str(&updated).unwrap();
        let (table, key) =
            if development { ("dependency-groups", "dev") } else { ("project", "dependencies") };
        assert_eq!(parsed[table][key][0].as_str(), Some("alpha==1.0"));
        python(root.path()).args(["-c", "import alpha"]).assert().success();
    }
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
