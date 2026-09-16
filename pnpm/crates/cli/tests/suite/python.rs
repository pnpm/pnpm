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
    path::{Path, PathBuf},
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
    files.extend(
        extra
            .iter()
            .map(|(path, contents)| (path.to_string(), contents.to_string())),
    );
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
        archive
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
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
    let versions = versions
        .iter()
        .map(|(version, archive)| (*version, archive.clone(), None))
        .collect::<Vec<_>>();
    serve_files(server, name, &versions, authorization).await
}

async fn serve_files(
    server: &mut mockito::ServerGuard,
    name: &str,
    versions: &[(&str, Vec<u8>, Option<&str>)],
    authorization: Option<&str>,
) -> Vec<mockito::Mock> {
    let mut mocks = Vec::new();
    let mut files = Vec::new();
    for (version, archive, requires_python) in versions {
        let filename = format!("{name}-{version}-py3-none-any.whl");
        files.push(json!({"filename": filename, "url": format!("/files/{filename}"), "hashes": {"sha256": format!("{:x}", Sha256::digest(archive))}, "requires-python": requires_python}));
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
    fs::write(root.join("pnpm-workspace.yaml"), format!("python:\n  enabled: true\n  indexUrl: '{index}/simple/'\nstoreDir: '{}'\ncacheDir: '{}'\nfetchRetries: 0\nallowBuilds:\n  pkg:pypi/hatchling: true\n  pkg:pypi/setuptools: true\n  pkg:pypi/wheel: true\n  pkg:pypi/tinybackend: true\n", root.join("store").display(), root.join("cache").display())).unwrap();
    fs::write(root.join("pyproject.toml"), format!("[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\ndependencies = {dependencies:?}\n")).unwrap();
}

async fn serve_wheels(
    server: &mut mockito::ServerGuard,
    name: &str,
    version: &str,
    tags: &[&str],
) -> Vec<mockito::Mock> {
    let mut mocks = Vec::new();
    let mut files = Vec::new();
    for tag in tags {
        let archive = wheel_with_tags(name, version, "", &[], &format!("Tag: {tag}\n"));
        let filename = format!("{name}-{version}-{tag}.whl");
        files.push(json!({"filename": filename, "url": format!("/files/{filename}"), "hashes": {"sha256": format!("{:x}", Sha256::digest(&archive))}}));
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

fn add_python_settings(root: &Path, settings: &str) {
    let workspace = fs::read_to_string(root.join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.join("pnpm-workspace.yaml"),
        workspace.replace(
            "python:\n  enabled: true\n",
            &format!("python:\n  enabled: true\n{settings}"),
        ),
    )
    .unwrap();
}

/// The platform of the machine running the tests, as `python.platforms`
/// names it, and the tag a wheel built for it carries.
fn running_platform() -> (&'static str, &'static str) {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => ("x86_64-manylinux_2_17", "py3-none-manylinux_2_17_x86_64"),
        ("linux", "aarch64") => ("aarch64-manylinux_2_17", "py3-none-manylinux_2_17_aarch64"),
        ("macos", "x86_64") => ("x86_64-apple-darwin", "py3-none-macosx_11_0_x86_64"),
        ("macos", "aarch64") => ("aarch64-apple-darwin", "py3-none-macosx_11_0_arm64"),
        ("windows", "x86_64") => ("x86_64-pc-windows-msvc", "py3-none-win_amd64"),
        ("windows", "aarch64") => ("aarch64-pc-windows-msvc", "py3-none-win_arm64"),
        (os, architecture) => panic!("these tests do not name the platform {os} {architecture}"),
    }
}

/// The short name `python.platforms` also accepts for the platform
/// running the test, when one of them stands for it.
fn running_platform_alias() -> Option<&'static str> {
    match running_platform().0 {
        "x86_64-manylinux_2_17" => Some("linux"),
        "aarch64-apple-darwin" => Some("macos"),
        "x86_64-pc-windows-msvc" => Some("windows"),
        _ => None,
    }
}

/// The platform running the test is always among these: an install
/// refuses an interpreter no declared environment stands for.
fn declared_platforms() -> Vec<(&'static str, &'static str)> {
    let mut platforms = vec![running_platform()];
    for platform in [
        ("aarch64-apple-darwin", "py3-none-macosx_11_0_arm64"),
        ("x86_64-pc-windows-msvc", "py3-none-win_amd64"),
    ] {
        if !platforms.contains(&platform) {
            platforms.push(platform);
        }
    }
    platforms
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
    assert!(flatten_report(&stderr).contains(&flatten_report(expected)), "expected: {expected}");
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
    pacquet_in(root.path())
        .args(["install", "--offline"])
        .assert()
        .success();
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
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
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
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    for directory in ["app-one", "app-two"] {
        python(&root.path().join(directory))
            .args(["-c", "import alpha"])
            .assert()
            .success();
        assert!(
            root.path()
                .join(directory)
                .join("pylock.toml")
                .exists(),
        );
    }
    for directory in [".venv/ignored", ".pnpm/ignored"] {
        assert!(
            !root
                .path()
                .join(directory)
                .join("pylock.toml")
                .exists(),
        );
    }
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
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha, beta; assert beta.VERSION == '1.0'"])
        .assert()
        .success();
    let command = root
        .path()
        .join(if cfg!(windows) { ".venv/Scripts/alpha-cli.cmd" } else { ".venv/bin/alpha-cli" });
    Command::new(command)
        .assert()
        .success()
        .stdout(if cfg!(windows) { "1.0\r\n" } else { "1.0\n" });
    assert_eq!(fs::read_to_string(root.path().join(".venv/share/alpha.txt")).unwrap(), "data file");
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&lock).unwrap();
    assert_eq!(parsed["lock-version"].as_str(), Some("1.0"));
    assert_eq!(
        parsed["packages"]
            .as_array()
            .unwrap()
            .len(),
        2,
    );
    assert!(
        !root
            .path()
            .join("pnpm-lock.yaml")
            .exists(),
    );
    assert!(
        !root
            .path()
            .join("package.json")
            .exists(),
    );
    for mock in alpha.into_iter().chain(beta) {
        mock.assert_async().await;
    }
    drop(server);
    pnpm_fs::remove_symlink_dir(&root.path().join(".venv")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha, beta"])
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["exec", "python", "-c", "import alpha, beta"])
        .assert()
        .success();
    let replayed_lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("INITIAL LOCK:\n{lock}\nREPLAYED LOCK:\n{replayed_lock}");
    assert_eq!(lock, replayed_lock);
}

/// The scenario of pnpm/pnpm#14843: an interpreter wrapper that reports the
/// kernel release `PNPM_TEST_KERNEL_RELEASE` names, with nothing else about
/// the interpreter, its wheel tags, or the project changing between runs.
/// An interpreter on the PATH under `name` that reports `version` while
/// running the real one, so a selection can be observed on a machine that
/// does not have that version.
#[cfg(unix)]
fn interpreter_shim(directory: &Path, name: &str, version: &str) {
    use std::os::unix::fs::PermissionsExt;
    // By the interpreter's own path: a shim named `python3` on the PATH it
    // is started through would otherwise run itself.
    let interpreter = Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    let interpreter = String::from_utf8(interpreter.stdout).unwrap();
    fs::create_dir_all(directory).unwrap();
    let shim = directory.join(name);
    fs::write(
        &shim,
        format!(
            concat!(
                "#!{interpreter}\n",
                "import platform, sys\n",
                "args = sys.argv[1:]\n",
                "if args and args[0] == '-I':\n",
                "    args = args[1:]\n",
                "assert len(args) >= 2 and args[0] == '-c'\n",
                "platform.python_version = lambda: '{version}'\n",
                "platform.python_version_tuple = lambda: tuple('{version}'.split('.'))\n",
                "sys.argv = ['-c', *args[2:]]\n",
                "exec(compile(args[1], '<pnpm-shim>', 'exec'), {{'__name__': '__main__'}})\n",
            ),
            interpreter = interpreter.trim(),
            version = version,
        ),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn selected_python(root: &Path) -> String {
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("pylock.toml")).unwrap()).unwrap();
    lock["tool"]["pnpm"]["environment"]["python_full_version"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The interpreter scenarios of pnpm/pnpm#14945: a project the machine's
/// default interpreter is too new for, and a `.python-version` file.
#[cfg(unix)]
#[tokio::test]
async fn selects_an_interpreter_the_project_accepts_and_its_python_version_file_asks_for() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "https://unused.invalid", &[]);
    // Outside the workspace, which an install does not take interpreters from.
    let outside = tempfile::tempdir().unwrap();
    let shims = outside.path().join("interpreters");
    interpreter_shim(&shims, "python3.11", "3.11.9");
    interpreter_shim(&shims, "python3.12", "3.12.7");
    let install = |requires_python: &str| {
        fs::write(
            root.path().join("pyproject.toml"),
            format!(
                "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '{requires_python}'\ndependencies = []\n",
            ),
        )
        .unwrap();
        let mut command = pacquet_in(root.path());
        command
            .args(["install", "--offline"])
            .env("PATH", format!("{}:{}", shims.display(), std::env::var("PATH").unwrap()));
        command
    };

    install("==3.11.9").assert().success();
    assert_eq!(selected_python(root.path()), "3.11.9");

    fs::write(root.path().join(".python-version"), "3.12\n").unwrap();
    install(">=3.11").assert().success();
    assert_eq!(selected_python(root.path()), "3.12.7");

    fs::write(root.path().join(".python-version"), "3.7\n").unwrap();
    let output = install(">=3.11").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("stdout:\n{stdout}\nstderr:\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    assert!(stdout.contains("asks for Python 3.7, which was not found"), "{stdout}");
    assert_ne!(selected_python(root.path()), "3.7");

    // The range is the project's requirement, so no interpreter means no install.
    fs::remove_file(root.path().join(".python-version")).unwrap();
    assert_failure_contains(&mut install("==3.99"), "no Python interpreter for");

    // A dependency's own bin directory can be on the PATH of an install that
    // runs from a script, so no interpreter is resolved through the workspace,
    // whether it is named there or linked from there.
    let bin = root.path().join("node_modules/.bin");
    interpreter_shim(&bin, "python3", "3.13.99");
    interpreter_shim(&outside.path().join("linked"), "python3.13", "3.13.99");
    std::os::unix::fs::symlink(outside.path().join("linked/python3.13"), bin.join("python3.13"))
        .unwrap();
    // The empty PATH entry below names the directory the install runs in,
    // which is the workspace this one is planted in.
    interpreter_shim(root.path(), "python3", "3.13.99");
    let mut planted = install("==3.13.99");
    planted.env("PATH", format!(":{}:{}", bin.display(), std::env::var("PATH").unwrap()));
    assert_failure_contains(&mut planted, "no Python interpreter for");

    // A configured interpreter is the only one, so nothing is searched for.
    add_python_settings(
        root.path(),
        &format!("  executable: '{}'\n", shims.join("python3.11").display()),
    );
    assert_failure_contains(
        &mut install("==3.12.7"),
        "requires Python ==3.12.7, but 3.11.9 was selected",
    );
}

#[cfg(unix)]
#[tokio::test]
async fn frozen_lockfile_replays_after_a_kernel_only_marker_change() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta; platform_release >= '9'", &[]))],
    )
    .await;
    let _beta = serve(&mut server, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    let probe = root.path().join("python-probe");
    fs::write(
        &probe,
        concat!(
            "#!/usr/bin/env python3\n",
            "import os, platform, sys\n",
            "args = sys.argv[1:]\n",
            "if args and args[0] == '-I':\n",
            "    args = args[1:]\n",
            "assert len(args) >= 2 and args[0] == '-c'\n",
            "platform.release = lambda: os.environ['PNPM_TEST_KERNEL_RELEASE']\n",
            "sys.argv = ['-c', *args[2:]]\n",
            "exec(compile(args[1], '<pnpm-probe>', 'exec'), {'__name__': '__main__'})\n",
        ),
    )
    .unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        workspace.replace(
            "python:\n  enabled: true\n",
            &format!("python:\n  enabled: true\n  executable: '{}'\n", probe.display()),
        ),
    )
    .unwrap();
    let install = |args: &[&str], kernel: &str| {
        let mut command = pacquet_in(root.path());
        command.args(args).env("PNPM_TEST_KERNEL_RELEASE", kernel);
        command
    };
    install(&["install"], "1.0.0").assert().success();
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("LOCK:\n{lock}");
    let parsed: toml::Value = toml::from_str(&lock).unwrap();
    let environments = parsed["environments"].as_array().unwrap();
    assert_eq!(environments.len(), 1);
    let marker = environments[0].as_str().unwrap();
    assert!(marker.contains("platform_release == '1.0.0'"), "{marker}");
    assert!(marker.contains("python_version == '"), "{marker}");
    assert!(!marker.contains("platform_version"), "{marker}");
    assert!(!marker.contains("sys_platform"), "{marker}");
    assert_eq!(
        parsed["packages"]
            .as_array()
            .unwrap()
            .len(),
        1,
    );

    pnpm_fs::remove_symlink_dir(&root.path().join(".venv")).unwrap();
    install(&["install", "--offline", "--frozen-lockfile"], "1.0.1").assert().success();
    assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();

    assert_failure_contains(
        &mut install(&["install", "--offline", "--frozen-lockfile"], "9.0.0"),
        "Python lockfile does not satisfy the project",
    );
    assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);

    let output = install(&["install"], "9.0.0").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("stdout:\n{stdout}\nstderr:\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    assert!(stdout.contains("[WARN] Ignoring Python lockfile"), "{stdout}");
    let relocked: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pylock.toml")).unwrap()).unwrap();
    assert_eq!(
        relocked["packages"]
            .as_array()
            .unwrap()
            .len(),
        2,
    );
    assert!(
        relocked["environments"][0]
            .as_str()
            .unwrap()
            .contains("platform_release == '9.0.0'"),
        "{relocked}",
    );
    python(root.path())
        .args(["-c", "import alpha, beta"])
        .assert()
        .success();
}

/// The scenario of pnpm/pnpm#14910: a release whose `Requires-Python` is
/// not a version specifier, both in the index page that lists it and in
/// the wheel's own metadata. Releases are immutable, so a project that
/// depends on one has no way to correct it.
#[tokio::test]
async fn installs_a_release_whose_requires_python_does_not_parse() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve_files(
        &mut server,
        "alpha",
        &[
            ("1.0", wheel("alpha", "1.0", "Requires-Python: >=3.6,", &[]), Some(">=3.6,")),
            ("2.0", wheel("alpha", "2.0", "Requires-Python: >=3.10", &[]), Some(">=3.10")),
        ],
        None,
    )
    .await;
    project(root.path(), &server.url(), &["alpha==1.0"]);

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    python(root.path())
        .args(["-c", "import alpha; assert alpha.VERSION == '1.0'"])
        .assert()
        .success();
}

#[tokio::test]
async fn add_updates_pyproject_and_lockfile_without_creating_node_metadata() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    pacquet_in(root.path())
        .args(["add", "pypi:alpha@>=1", "--save-dev"])
        .assert()
        .success();
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pyproject.toml")).unwrap()).unwrap();
    assert_eq!(manifest["dependency-groups"]["dev"][0].as_str(), Some("alpha>=1"));
    assert!(
        !root
            .path()
            .join("package.json")
            .exists(),
    );
    assert!(!root.path().join("Cargo.toml").exists());
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();
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
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();
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
        index
            .set_password(Some(password))
            .unwrap();
        project(root.path(), index.as_str().trim_end_matches('/'), &["alpha"]);
        let workspace_path = root.path().join("pnpm-workspace.yaml");
        let workspace = fs::read_to_string(&workspace_path).unwrap();
        fs::write(workspace_path, workspace.replace("/simple/'", "/simple'")).unwrap();
        pacquet_in(root.path())
            .arg("install")
            .assert()
            .success();
        let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        eprintln!("lockfile:\n{lock}");
        assert!(!lock.contains(password), "credentials leaked into lockfile");
        python(root.path())
            .args(["-c", "import alpha"])
            .assert()
            .success();
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
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();
    index.assert_async().await;
    download.assert_async().await;
}

#[tokio::test]
async fn caches_python_index_as_raw_json_and_reuses_it_offline() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha"]);
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let cache = fs::read_dir(root.path().join("cache/python-index-v2"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let cached: serde_json::Value = serde_json::from_slice(&fs::read(cache).unwrap()).unwrap();
    dbg!(&cached);
    assert!(cached["body"]["files"].is_array(), "metadata was not stored as a JSON object");
    pacquet_in(root.path())
        .args(["add", "pypi:alpha", "--offline"])
        .assert()
        .success();
}

#[tokio::test]
async fn frozen_wheel_downloads_replenish_slots_and_settle_before_reporting_failure() {
    for fail in [false, true] {
        eprintln!("fail={fail}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let archives = [
            ("alpha", wheel("alpha", "1.0", "", &[])),
            ("beta", wheel("beta", "1.0", "", &[])),
            ("gamma", wheel("gamma", "1.0", "", &[])),
        ];
        let mut initial_requests = Vec::new();
        for (name, archive) in &archives {
            initial_requests.extend(serve(&mut server, name, &[("1.0", archive.clone())]).await);
        }
        project(root.path(), &server.url(), &["alpha", "beta", "gamma"]);
        pacquet_in(root.path())
            .args(["install", "--lockfile-only"])
            .assert()
            .success();
        for request in initial_requests {
            request.remove_async().await;
        }
        let before = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        let rendezvous = Arc::new((Mutex::new(0), Condvar::new()));
        let sibling_finished = Arc::new(AtomicBool::new(false));
        let downloads =
            mock_wheel_downloads(&mut server, archives, fail, &rendezvous, &sibling_finished).await;
        let mut command = pacquet_in(root.path());
        command
            .env("PNPM_CONFIG_STORE_DIR", root.path().join("cold-store"))
            .env("PNPM_CONFIG_NETWORK_CONCURRENCY", "2")
            .args(["install", "--frozen-lockfile"]);
        assert_frozen_install_outcome(&mut command, root.path(), fail);
        assert!(sibling_finished.load(Ordering::SeqCst), "returned before sibling body finished");
        let after = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        eprintln!("INITIAL LOCK:\n{before}\nREPLAYED LOCK:\n{after}");
        assert_eq!(before, after);
        for request in downloads {
            request.assert_async().await;
        }
    }
}

fn assert_frozen_install_outcome(command: &mut Command, root: &Path, fail: bool) {
    if fail {
        command.assert().failure();
        assert!(!root.join(".venv").exists(), "published a failed environment");
        return;
    }
    command.assert().success();
    python(root)
        .args(["-c", "import alpha, beta, gamma"])
        .assert()
        .success();
}

/// One rendezvous-gated download mock per wheel, so every download is in
/// flight before any of them completes.
async fn mock_wheel_downloads(
    server: &mut mockito::ServerGuard,
    archives: [(&'static str, Vec<u8>); 3],
    fail: bool,
    rendezvous: &Arc<(Mutex<usize>, Condvar)>,
    sibling_finished: &Arc<AtomicBool>,
) -> Vec<mockito::Mock> {
    let mut downloads = Vec::new();
    for (name, archive) in archives {
        downloads.push(
            server
                .mock("GET", format!("/files/{name}-1.0-py3-none-any.whl").as_str())
                .with_chunked_body(wheel_download_body(
                    name,
                    archive,
                    fail,
                    Arc::clone(rendezvous),
                    Arc::clone(sibling_finished),
                ))
                .expect(1)
                .create_async()
                .await,
        );
    }
    downloads
}

/// The body one wheel download serves. Under `fail`, `alpha` serves a
/// corrupt archive while its siblings are still writing theirs.
fn wheel_download_body(
    name: &'static str,
    archive: Vec<u8>,
    fail: bool,
    rendezvous: Arc<(Mutex<usize>, Condvar)>,
    sibling_finished: Arc<AtomicBool>,
) -> impl Fn(&mut dyn Write) -> std::io::Result<()> + Send + Sync + 'static {
    move |writer| {
        await_every_download(name, &rendezvous)?;
        if fail && name == "alpha" {
            return writer.write_all(b"corrupt wheel");
        }
        if fail {
            std::thread::sleep(Duration::from_millis(250));
        }
        writer.write_all(&archive)?;
        if name == "gamma" {
            sibling_finished.store(true, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// Count this download in, and — for `alpha`, which holds a slot while it
/// waits — block until all three have started. Timing out means the slot
/// `alpha` holds was never replenished.
fn await_every_download(name: &str, rendezvous: &(Mutex<usize>, Condvar)) -> std::io::Result<()> {
    let (arrivals, wake) = rendezvous;
    let mut arrivals = arrivals.lock().unwrap();
    *arrivals += 1;
    wake.notify_all();
    if name != "alpha" {
        return Ok(());
    }
    let (arrivals, timeout) = wake
        .wait_timeout_while(arrivals, Duration::from_secs(10), |arrivals| *arrivals < 3)
        .unwrap();
    drop(arrivals);
    if timeout.timed_out() {
        return Err(std::io::Error::other("wheel download slot was not replenished"));
    }
    Ok(())
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
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
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
            pacquet_in(root.path())
                .args(["add", "pypi:alpha"])
                .args(flags),
            expected,
        );
    }
}

#[test]
fn invalid_python_save_prefix_is_rejected_before_manifest_parsing_or_interpreter_start() {
    for contents in ["[project]\nname = 'app'\ndependencies = []\n", "not valid TOML"] {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("pnpm-workspace.yaml"),
            "python:\n  enabled: true\n  executable: this-interpreter-does-not-exist\n",
        )
        .unwrap();
        let manifest = root.path().join("pyproject.toml");
        fs::write(&manifest, contents).unwrap();
        assert_failure_contains(
            pacquet_in(root.path()).args(["add", "pypi:alpha", "--save-prefix=^"]),
            "Python --save-prefix must be >=, ~=, or ==",
        );
        assert_eq!(fs::read_to_string(manifest).unwrap(), contents);
        assert!(!root.path().join("pylock.toml").exists());
        assert!(!root.path().join(".venv").exists());
    }
}

fn python_add_failure(root: &Path) -> String {
    let result = pacquet_in(root)
        .args(["add", "pypi:alpha", "-w"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&result.get_output().stderr).into_owned();
    eprintln!("stderr:\n{stderr}");
    flatten_report(&stderr)
}

/// The last two components of a temporary project's manifest path. macOS
/// resolves the leading directories of `TMPDIR` and Windows rewrites their
/// case, so only the tail is the same in the diagnostic and in the test.
fn manifest_tail(root: &Path) -> PathBuf {
    Path::new(root.file_name().expect("the temporary directory has a name")).join("pyproject.toml")
}

#[test]
fn python_add_without_a_pyproject_toml_names_the_missing_manifest() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "https://unused.invalid", &[]);
    let manifest = root.path().join("pyproject.toml");
    fs::remove_file(&manifest).unwrap();
    let report = python_add_failure(root.path());
    assert!(report.contains(&flatten_report("cannot add a Python dependency because")), "{report}");
    let named = format!("{} does not exist", manifest_tail(root.path()).display());
    assert!(report.contains(&flatten_report(&named)), "{report}");
    let help =
        "help: Run the command in a directory that has a pyproject.toml, or create one there.";
    assert!(report.contains(&flatten_report(help)), "{report}");
    assert!(!manifest.exists());
    assert!(!root.path().join("pylock.toml").exists());
}

/// A `pyproject.toml` that is not a regular file exists, so the failure must
/// name it as what it is rather than as a missing manifest.
#[test]
fn python_add_does_not_report_a_present_pyproject_toml_as_missing() {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "https://unused.invalid", &[]);
    let manifest = root.path().join("pyproject.toml");
    fs::remove_file(&manifest).unwrap();
    fs::create_dir(&manifest).unwrap();
    let report = python_add_failure(root.path());
    let named = manifest_tail(root.path());
    assert!(report.contains(&flatten_report(&named.display().to_string())), "{report}");
    assert!(!report.contains(&flatten_report("does not exist")), "{report}");
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
        python(root.path())
            .args(["-c", "import alpha"])
            .assert()
            .success();
    }
}

#[tokio::test]
async fn add_pins_bare_requirements_and_preserves_unrelated_manifest_text() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::write(root.path().join("pyproject.toml"), "# keep this comment\n[project]\nname = 'app' # original quoting\nversion = '1.0'\n\n[tool.example]\nsetting = 'preserve'\n").unwrap();
    pacquet_in(root.path())
        .args(["add", "pypi:alpha", "--save-exact"])
        .assert()
        .success();
    let text = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
    assert!(text.starts_with("# keep this comment\n[project]"));
    assert!(text.contains("name = 'app' # original quoting"));
    assert!(text.contains("[tool.example]\nsetting = 'preserve'"));
    assert!(text.contains("alpha==1.0"));
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
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
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    assert!(
        root.path()
            .join("node_modules/local-node/package.json")
            .exists(),
    );
    assert!(
        root.path()
            .join("pnpm-lock.yaml")
            .exists(),
    );
    assert!(root.path().join("Cargo.lock").exists());
    assert!(root.path().join("pylock.toml").exists());
    Command::new("cargo")
        .current_dir(root.path())
        .args(["check", "--offline", "--locked"])
        .assert()
        .success();
    pacquet_in(root.path())
        .args(["run", "python-check"])
        .assert()
        .success();
}

/// The scenario of pnpm/pnpm#14945.
#[tokio::test]
async fn installs_the_projects_own_package_from_its_source_tree() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    fs::write(root.path().join("pyproject.toml"), "[project]\nname = 'My-App'\nversion = '1.2.3'\nrequires-python = '>=3.10'\ndependencies = ['alpha>=1']\n\n[project.scripts]\nmy-app = 'my_app:main'\n\n[project.entry-points.pnpm_demo]\nplugin = 'my_app:main'\n\n[build-system]\nrequires = ['hatchling']\nbuild-backend = 'hatchling.build'\n").unwrap();
    fs::create_dir(root.path().join("my_app")).unwrap();
    let source = root.path().join("my_app/__init__.py");
    fs::write(&source, "def main():\n    print('first')\n").unwrap();
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    // The environment directory holds no modules of its own, so an import
    // that succeeds there is one the installed package provides.
    python(root.path())
        .current_dir(root.path().join(".venv"))
        .args(["-c", "import alpha, my_app"])
        .assert()
        .success();
    python(root.path())
        .args([
            "-c",
            "import importlib.metadata as m; d = m.distribution('my-app'); assert d.version == '1.2.3', d.version; assert 'alpha>=1' in m.requires('my-app'), m.requires('my-app'); assert d.read_text('INSTALLER').strip() == 'pnpm'; import json; assert json.loads(d.read_text('direct_url.json'))['dir_info']['editable']; assert [entry.value for entry in m.entry_points(group='pnpm_demo')] == ['my_app:main']",
        ])
        .assert()
        .success();

    let script = root
        .path()
        .join(if cfg!(windows) { ".venv/Scripts/my-app.cmd" } else { ".venv/bin/my-app" });
    Command::new(&script)
        .assert()
        .success()
        .stdout(if cfg!(windows) { "first\r\n" } else { "first\n" });
    fs::write(&source, "def main():\n    print('second')\n").unwrap();
    Command::new(&script)
        .assert()
        .success()
        .stdout(if cfg!(windows) { "second\r\n" } else { "second\n" });
}

#[tokio::test]
async fn installs_the_package_of_a_src_layout_project_and_none_for_a_virtual_one() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::write(root.path().join("pyproject.toml"), "[tool.ruff]\nline-length = 100\n").unwrap();
    for (directory, module, manifest) in [
        (
            "packaged",
            "src/packaged",
            "[project]\nname = 'packaged'\nversion = '0.1'\ndependencies = ['alpha>=1']\n\n[build-system]\nrequires = ['setuptools']\n",
        ),
        (
            "declared",
            "modules/declared",
            "[project]\nname = 'declared'\nversion = '0.1'\ndependencies = []\n\n[build-system]\nrequires = ['hatchling']\n\n[tool.hatch.build.targets.wheel]\npackages = ['modules/declared']\n",
        ),
        ("virtual", "virtual", "[project]\nname = 'virtual'\nversion = '0.1'\ndependencies = []\n"),
    ] {
        let path = root.path().join(directory);
        fs::create_dir_all(path.join(module)).unwrap();
        fs::write(path.join("pyproject.toml"), manifest).unwrap();
        fs::write(path.join(module).join("__init__.py"), "VALUE = 'source'\n").unwrap();
    }
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    for (directory, module) in [("packaged", "packaged"), ("declared", "declared")] {
        let project = root.path().join(directory);
        python(&project)
            .current_dir(project.join(".venv"))
            .args(["-c", &format!("import {module}; assert {module}.VALUE == 'source'")])
            .assert()
            .success();
    }
    python(&root.path().join("packaged"))
        .args(["-c", "import alpha"])
        .assert()
        .success();
    // A project no build backend builds has no package to install.
    python(&root.path().join("virtual"))
        .args(["-c", "import importlib.metadata as m; m.distribution('virtual')"])
        .assert()
        .failure();
}

#[tokio::test]
async fn a_dynamic_version_comes_from_the_backend() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\ndynamic = ['version']\nrequires-python = '>=3.10'\n\
         dependencies = ['alpha>=1']\n\n[build-system]\nrequires = ['hatchling']\n\
         build-backend = 'hatchling.build'\n",
    )
    .unwrap();
    fs::create_dir(root.path().join("app")).unwrap();
    fs::write(root.path().join("app/__init__.py"), "VALUE = 'source'\n").unwrap();

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    python(root.path())
        .args(["-c", "import importlib.metadata as m; print(m.version('app'))"])
        .assert()
        .success()
        .stdout(if cfg!(windows) { "0.0.1\r\n" } else { "0.0.1\n" });
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
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "python:\n  enabled: true\n  executable: this-interpreter-does-not-exist\n",
    )
    .unwrap();
    fs::write(root.path().join("pyproject.toml"), "[tool.ruff]\nline-length = 100\n").unwrap();
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    assert!(!root.path().join("pylock.toml").exists());
}

/// Blocker 3 of pnpm/pnpm#14945: a committed lockfile has to serve every
/// machine that installs from it, not only the one that resolved it.
#[tokio::test]
async fn locks_every_declared_platform_into_one_lockfile() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let platforms = declared_platforms();
    let _alpha = serve(
        &mut server,
        "alpha",
        &[("1.0", wheel("alpha", "1.0", "Requires-Dist: beta; sys_platform == 'win32'", &[]))],
    )
    .await;
    let _beta = serve(&mut server, "beta", &[("1.0", wheel("beta", "1.0", "", &[]))]).await;
    let _gamma = serve_wheels(
        &mut server,
        "gamma",
        "1.0",
        &platforms
            .iter()
            .map(|(_, tag)| *tag)
            .collect::<Vec<_>>(),
    )
    .await;
    project(root.path(), &server.url(), &["alpha>=1", "gamma>=1"]);
    let mut declaration = String::new();
    for (platform, _) in &platforms {
        writeln!(declaration, "    - {platform}").unwrap();
    }
    add_python_settings(root.path(), &format!("  platforms:\n{declaration}"));

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("LOCK:\n{lock}");
    let parsed: toml::Value = toml::from_str(&lock).unwrap();
    let environments = parsed["environments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|marker| marker.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(environments.len(), platforms.len());
    assert!(
        environments
            .iter()
            .any(|marker| marker.contains("sys_platform == 'win32'")
                && marker.contains("platform_machine == 'AMD64'")),
        "{environments:?}",
    );
    assert!(
        environments
            .iter()
            .any(|marker| marker.contains("sys_platform == 'darwin'")
                && marker.contains("platform_machine == 'arm64'")),
        "{environments:?}",
    );
    let packages = parsed["packages"].as_array().unwrap();
    let package = |name: &str| {
        packages
            .iter()
            .find(|package| package["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("{name} is locked"))
    };
    assert!(package("alpha").get("marker").is_none(), "{:?}", package("alpha"));
    assert!(
        package("beta")["marker"]
            .as_str()
            .unwrap()
            .contains("sys_platform == 'win32'"),
        "{:?}",
        package("beta"),
    );
    assert!(package("gamma").get("marker").is_none(), "{:?}", package("gamma"));
    assert_eq!(
        package("gamma")["wheels"]
            .as_array()
            .unwrap()
            .len(),
        platforms.len(),
    );

    python(root.path())
        .args(["-c", "import alpha, gamma"])
        .assert()
        .success();
    let beta = python(root.path())
        .args(["-c", "import beta"])
        .assert();
    if cfg!(windows) {
        beta.success();
    } else {
        beta.failure();
    }

    pnpm_fs::remove_symlink_dir(&root.path().join(".venv")).unwrap();
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
    python(root.path())
        .args(["-c", "import alpha, gamma"])
        .assert()
        .success();
}

/// A lockfile resolved for declared environments says nothing about an
/// interpreter none of them stand for.
#[tokio::test]
async fn refuses_an_interpreter_none_of_the_declared_environments_stand_for() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    add_python_settings(root.path(), "  pythonVersions:\n    - '3.9'\n");

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "is not one of the environments this project locks for",
    );
}

/// A name pnpm cannot resolve for is a typo, not a platform whose wheels
/// are all missing.
#[tokio::test]
async fn rejects_a_platform_it_cannot_resolve_for() {
    let cases = [
        ("x86_64-linux", "pnpm does not know the Python platform x86_64-linux"),
        (
            "x86_64-manylinux_2_100000000",
            "pnpm does not know the Python libc baseline manylinux_2_100000000",
        ),
        ("x86_64-musllinux_9_9", "pnpm does not know the Python libc baseline musllinux_9_9"),
    ];
    for (platform, expected) in cases {
        eprintln!("platform {platform:?}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
        project(root.path(), &server.url(), &["alpha>=1"]);
        add_python_settings(root.path(), &format!("  platforms:\n    - {platform}\n"));

        assert_failure_contains(pacquet_in(root.path()).arg("install"), expected);
    }
}

/// Dropping a repeat leaves a lockfile that still answers the project:
/// what it records is the environments it resolved, not the spellings.
#[tokio::test]
async fn locks_one_environment_per_platform_however_it_is_named() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let (platform, tag) = running_platform();
    let _alpha = serve_wheels(&mut server, "alpha", "1.0", &[tag]).await;
    let alias = running_platform_alias();
    let names = |repeated: bool| {
        let mut declaration = String::new();
        for _ in 0..if repeated { 2 } else { 1 } {
            writeln!(declaration, "    - {platform}").unwrap();
        }
        if let Some(alias) = alias {
            writeln!(declaration, "    - {alias}").unwrap();
        }
        format!("  platforms:\n{declaration}")
    };
    project(root.path(), &server.url(), &["alpha>=1"]);
    add_python_settings(root.path(), &names(true));

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("LOCK:\n{lock}");
    let parsed: toml::Value = toml::from_str(&lock).unwrap();
    assert_eq!(
        parsed["environments"]
            .as_array()
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(
        parsed["tool"]["pnpm"]["platforms"]
            .as_array()
            .unwrap()
            .len(),
        1 + usize::from(alias.is_some()),
    );

    project(root.path(), &server.url(), &["alpha>=1"]);
    add_python_settings(root.path(), &names(false));
    pacquet_in(root.path())
        .args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
}

mod metadata;
mod selection;

mod sources;
mod validation;

/// A PEP 517 backend small enough to serve from the mocked index, so a
/// build in these tests runs the real hooks without a real backend.
///
/// It prints on the way through: a backend writing to stdout must not
/// reach the JSON the host helper answers pnpm with.
const TINY_BACKEND: &str = r#"
import base64, hashlib, os, sys, tomllib, zipfile

def _source_dir(manifest):
    """Where the modules are, the way a backend knows and a manifest reader cannot."""
    wheel = manifest.get("tool", {}).get("hatch", {}).get("build", {}).get("targets", {}).get("wheel", {})
    declared = wheel.get("packages")
    if declared:
        return os.path.dirname(declared[0]) or "."
    return "src" if os.path.isdir("src") else "."

def _entry_points(project):
    groups = dict(project.get("entry-points", {}))
    for group, table in (("console_scripts", "scripts"), ("gui_scripts", "gui-scripts")):
        if project.get(table):
            groups[group] = project[table]
    return groups

def _build(directory, editable):
    print("building", file=sys.stdout)
    manifest = tomllib.load(open("pyproject.toml", "rb"))
    project = manifest["project"]
    name = project["name"]
    module = name.replace("-", "_")
    # A project may leave its version to the backend, as `dynamic` does.
    version = project.get("version", "0.0.1")
    dist_info = module + "-" + version + ".dist-info"
    entries = {}
    if editable:
        entries["_editable_" + module + ".pth"] = os.path.join(os.getcwd(), _source_dir(manifest))
    else:
        source = os.path.join(_source_dir(manifest), module, "__init__.py")
        entries[module + "/__init__.py"] = open(source).read()
    metadata = "Metadata-Version: 2.4\nName: " + name + "\nVersion: " + version + "\n"
    if "requires-python" in project:
        metadata += "Requires-Python: " + project["requires-python"] + "\n"
    for requirement in project.get("dependencies", []):
        metadata += "Requires-Dist: " + requirement + "\n"
    entries[dist_info + "/METADATA"] = metadata
    entries[dist_info + "/WHEEL"] = "Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n"
    declaration = ""
    for group, table in _entry_points(project).items():
        declaration += "[" + group + "]\n"
        for key, value in table.items():
            declaration += key + " = " + value + "\n"
    if declaration:
        entries[dist_info + "/entry_points.txt"] = declaration
    record = ""
    for path, body in entries.items():
        digest = base64.urlsafe_b64encode(hashlib.sha256(body.encode()).digest()).rstrip(b"=").decode()
        record += path + ",sha256=" + digest + "," + str(len(body.encode())) + "\n"
    record += dist_info + "/RECORD,,\n"
    entries[dist_info + "/RECORD"] = record
    filename = module + "-" + version + "-py3-none-any.whl"
    with zipfile.ZipFile(os.path.join(directory, filename), "w") as archive:
        for path, body in entries.items():
            archive.writestr(path, body)
    return filename

def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    return _build(wheel_directory, False)

def build_editable(wheel_directory, config_settings=None, metadata_directory=None):
    return _build(wheel_directory, True)
"#;

/// The backends the fixtures declare, served so a build can run. What
/// these tests exercise is the build frontend; which wheel a real backend
/// would produce is that backend's own business.
async fn serve_backends(server: &mut mockito::ServerGuard) -> Vec<mockito::Mock> {
    let mut mocks = Vec::new();
    // PEP 517 names setuptools' legacy backend when a project declares
    // none, and reads it as an attribute of the module rather than the
    // module itself.
    let legacy = format!("{TINY_BACKEND}\nimport sys\n__legacy__ = sys.modules[__name__]\n");
    for (name, module, source) in [
        ("hatchling", "hatchling/build.py", TINY_BACKEND),
        ("setuptools", "setuptools/build_meta.py", legacy.as_str()),
        ("wheel", "wheel/_unused.py", ""),
        ("tinybackend", "tinybuild.py", TINY_BACKEND),
    ] {
        mocks.extend(
            serve(server, name, &[("80.0", wheel(name, "80.0", "", &[(module, source)]))]).await,
        );
    }
    mocks
}

fn python_project(root: &Path, name: &str, body: &str) {
    fs::create_dir_all(root.join("src").join(name)).unwrap();
    fs::write(
        root.join("src")
            .join(name)
            .join("__init__.py"),
        format!("MARKER = 'workspace {name}'\n"),
    )
    .unwrap();
    fs::write(
        root.join("pyproject.toml"),
        format!(
            "[project]\nname = '{name}'\nversion = '1.0'\nrequires-python = '>=3.10'\n{body}\n\
             [build-system]\nrequires = ['tinybackend']\nbuild-backend = 'tinybuild'\n",
        ),
    )
    .unwrap();
}

/// The index publishes a different `mylib`, so which one is installed is
/// what this is about.
#[tokio::test]
async fn a_workspace_project_is_built_from_its_source_instead_of_the_index() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _mylib = serve(&mut server, "mylib", &[("9.0", wheel("mylib", "9.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    python_project(&root.path().join("packages/mylib"), "mylib", "dependencies = []");
    python_project(
        &root.path().join("packages/app"),
        "app",
        "dependencies = ['mylib']\n\n[tool.uv.sources]\nmylib = { workspace = true }\n",
    );

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    let app = root.path().join("packages/app");
    python(&app)
        .args(["-c", "import mylib; print(mylib.MARKER)"])
        .assert()
        .success()
        .stdout(if cfg!(windows) { "workspace mylib\r\n" } else { "workspace mylib\n" });
    python(&app)
        .args(["-c", "import app; print(app.MARKER)"])
        .assert()
        .success()
        .stdout(if cfg!(windows) { "workspace app\r\n" } else { "workspace app\n" });
    let lock = fs::read_to_string(app.join("pylock.toml")).unwrap();
    assert!(lock.contains(r#"path = "../mylib""#), "records the project's path: {lock}");
    assert!(lock.contains("editable = true"), "installs it editable: {lock}");
    assert!(!lock.contains("9.0"), "never reaches the index for mylib: {lock}");
}

#[tokio::test]
async fn a_requirement_naming_a_workspace_project_is_not_taken_from_the_index() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _mylib = serve(&mut server, "mylib", &[("9.0", wheel("mylib", "9.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    python_project(&root.path().join("packages/mylib"), "mylib", "dependencies = []");
    python_project(&root.path().join("packages/app"), "app", "dependencies = ['mylib']");

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "names a project in this workspace",
    );
}

#[tokio::test]
async fn a_project_outside_the_declared_members_is_resolved_from_the_index() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _mylib = serve(&mut server, "mylib", &[("9.0", wheel("mylib", "9.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["mylib"]);
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['mylib']\n\n[tool.uv.workspace]\nmembers = ['packages/*']\n",
    )
    .unwrap();
    let vendored = root.path().join("vendor/mylib");
    fs::create_dir_all(&vendored).unwrap();
    fs::write(
        vendored.join("pyproject.toml"),
        "[project]\nname = 'mylib'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n",
    )
    .unwrap();

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    assert!(lock.contains("9.0"), "the index serves a distribution of that name: {lock}");
}

#[tokio::test]
async fn a_source_pnpm_cannot_resolve_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &[]);
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['talon-core']\n\n[tool.uv.sources]\n\
         talon-core = { index = 'private' }\n",
    )
    .unwrap();

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "does not support the index Python source",
    );
}

/// A path source may point outside the projects pnpm discovered, and the
/// project it names is not part of any workspace those projects form.
#[tokio::test]
async fn a_path_source_outside_the_discovered_projects_resolves() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    let _backends = serve_backends(&mut server).await;
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    project(&workspace, &server.url(), &[]);
    python_project(&root.path().join("outside/lib"), "lib", "dependencies = ['alpha']");
    fs::write(
        workspace.join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['lib']\n\n[tool.uv.sources]\n\
         lib = { path = '../outside/lib', editable = true }\n",
    )
    .unwrap();

    pacquet_in(&workspace)
        .arg("install")
        .assert()
        .success();
    python(&workspace)
        .args(["-c", "import lib; print(lib.MARKER)"])
        .assert()
        .success()
        .stdout(if cfg!(windows) { "workspace lib\r\n" } else { "workspace lib\n" });
}

/// A relative path means the same project to every member that inherits
/// the declaration, so it is read against the manifest that declared it.
#[tokio::test]
async fn an_inherited_path_source_resolves_against_the_workspace_root() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'root'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n\n[tool.uv.workspace]\nmembers = ['packages/*']\n\n\
         [tool.uv.sources]\nlib = { path = 'vendor/lib', editable = true }\n",
    )
    .unwrap();
    python_project(&root.path().join("packages/app"), "app", "dependencies = ['lib']");
    python_project(&root.path().join("vendor/lib"), "lib", "dependencies = []");

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let app = root.path().join("packages/app");
    python(&app)
        .args(["-c", "import lib; print(lib.MARKER)"])
        .assert()
        .success()
        .stdout(if cfg!(windows) { "workspace lib\r\n" } else { "workspace lib\n" });
    let lock = fs::read_to_string(app.join("pylock.toml")).unwrap();
    assert!(lock.contains(r#"path = "../../vendor/lib""#), "reads it against the root: {lock}");
}

/// Where a locked project's source is and how it is installed are not in
/// the solved graph, so a frozen install checks them itself.
#[tokio::test]
async fn a_frozen_install_refuses_a_workspace_project_that_moved() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    fs::remove_file(root.path().join("pyproject.toml")).unwrap();
    python_project(&root.path().join("packages/lib"), "lib", "dependencies = []");
    python_project(&root.path().join("elsewhere/lib"), "lib", "dependencies = []");
    let app = root.path().join("packages/app");
    let declare = |path: &str| {
        fs::write(
            app.join("pyproject.toml"),
            format!(
                "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
                 dependencies = ['lib']\n\n[tool.uv.sources]\n\
                 lib = {{ path = '{path}', editable = true }}\n\n\
                 [build-system]\nrequires = ['tinybackend']\nbuild-backend = 'tinybuild'\n",
            ),
        )
        .unwrap();
    };
    python_project(&app, "app", "dependencies = []");
    declare("../lib");
    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    declare("../../elsewhere/lib");
    assert_failure_contains(
        pacquet_in(root.path()).args(["install", "--frozen-lockfile"]),
        "frozen Python lockfile is missing or out of date",
    );
}

/// A backend builds the project it was handed, and installing anything
/// else would install what no lockfile describes.
#[tokio::test]
async fn a_backend_building_another_distribution_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[(
            "1.0",
            wheel(
                "tinybackend",
                "1.0",
                "",
                &[(
                    "tinybuild.py",
                    TINY_BACKEND
                        .replace(r#"name = project["name"]"#, r#"name = "impostor""#)
                        .as_str(),
                )],
            ),
        )],
    )
    .await;
    project(root.path(), &server.url(), &[]);
    python_project(root.path(), "app", "dependencies = []");

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "but its backend built `impostor`",
    );
}

/// A distribution the lockfile pins from the index may become a project
/// in the workspace without any requirement changing, which no comparison
/// of the requirements or the solved versions can show.
#[tokio::test]
async fn a_lockfile_pinning_an_index_wheel_gives_way_to_a_workspace_project() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _lib = serve(&mut server, "lib", &[("1.0", wheel("lib", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["lib"]);
    pacquet_in(root.path())
        .args(["install", "--lockfile-only"])
        .assert()
        .success();
    let locked = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    assert!(locked.contains("lib-1.0-py3-none-any.whl"), "starts pinned to the index: {locked}");

    // The same distribution, at the version the lockfile already pins, so
    // only where it comes from changes.
    let lib = root.path().join("packages/lib");
    fs::create_dir_all(&lib).unwrap();
    fs::write(
        lib.join("pyproject.toml"),
        "[project]\nname = 'lib'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n",
    )
    .unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['lib']\n\n[tool.uv.sources]\nlib = { workspace = true }\n",
    )
    .unwrap();

    pacquet_in(root.path())
        .args(["install", "--lockfile-only"])
        .assert()
        .success();
    let locked = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    assert!(locked.contains(r#"path = "packages/lib""#), "records the project: {locked}");
    assert!(!locked.contains("lib-1.0-py3-none-any.whl"), "drops the wheel: {locked}");
}

/// A project may leave its version to its backend, and the wheel is still
/// the project it was built from.
#[tokio::test]
async fn a_backend_building_another_name_is_refused_without_a_declared_version() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[(
            "1.0",
            wheel(
                "tinybackend",
                "1.0",
                "",
                &[(
                    "tinybuild.py",
                    TINY_BACKEND
                        .replace(r#"name = project["name"]"#, r#"name = "impostor""#)
                        .as_str(),
                )],
            ),
        )],
    )
    .await;
    project(root.path(), &server.url(), &[]);
    python_project(root.path(), "app", "dependencies = []");
    fs::create_dir(root.path().join("src/impostor")).unwrap();
    fs::write(root.path().join("src/impostor/__init__.py"), "").unwrap();
    // The backend names the version, so only the distribution is declared.
    fs::write(
        root.path().join("pyproject.toml"),
        fs::read_to_string(root.path().join("pyproject.toml"))
            .unwrap()
            .replace("version = '1.0'", "dynamic = ['version']"),
    )
    .unwrap();

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "but its backend built `impostor`",
    );
}

/// One project is installed one way. Reachable declarations that disagree
/// about that are a conflict, not a race between them.
#[tokio::test]
async fn sources_disagreeing_about_editable_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    python_project(&root.path().join("packages/lib"), "lib", "dependencies = []");
    python_project(
        &root.path().join("packages/helper"),
        "helper",
        "dependencies = ['lib']\n\n[tool.uv.sources]\n\
         lib = { path = '../lib', editable = false }\n",
    );
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['helper', 'lib']\n\n[tool.uv.sources]\n\
         helper = { workspace = true }\nlib = { path = 'packages/lib', editable = true }\n",
    )
    .unwrap();

    assert_failure_contains(pacquet_in(root.path()).arg("install"), "one editable, one not");
}

/// A source pnpm applies everywhere must not be one the manifest narrowed
/// to some targets, or to one extra or group.
#[tokio::test]
async fn a_source_narrowed_by_a_marker_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &[]);
    fs::create_dir_all(root.path().join("packages/lib")).unwrap();
    fs::write(
        root.path().join("packages/lib/pyproject.toml"),
        "[project]\nname = 'lib'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n",
    )
    .unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['lib']\n\n[tool.uv.sources]\n\
         lib = { workspace = true, marker = \"sys_platform == 'never'\" }\n",
    )
    .unwrap();

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "does not support the conditional Python source",
    );
}

/// A path source names where the project is, and the error says what was
/// declared rather than what reading it failed on.
#[tokio::test]
async fn a_path_source_naming_a_missing_directory_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &[]);
    fs::create_dir_all(root.path().join("lib")).unwrap();
    fs::write(
        root.path().join("lib/pyproject.toml"),
        "[project]\nname = 'lib'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n",
    )
    .unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['lib']\n\n[tool.uv.sources]\n\
         lib = { path = 'nowhere/lib' }\n",
    )
    .unwrap();

    assert_failure_contains(pacquet_in(root.path()).arg("install"), "which is not a directory");
}

/// A `..` after a symlink goes to the link's target on POSIX and to the
/// directory the path was written in on Windows, so a path source that
/// asks for one is refused rather than read as either.
#[cfg(unix)]
#[tokio::test]
async fn a_path_source_stepping_out_of_a_symlink_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &[]);
    for directory in ["lib", "sub/elsewhere", "sub/lib"] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
        fs::write(
            root.path()
                .join(directory)
                .join("pyproject.toml"),
            "[project]\nname = 'lib'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
             dependencies = []\n",
        )
        .unwrap();
    }
    std::os::unix::fs::symlink(root.path().join("sub/elsewhere"), root.path().join("link"))
        .unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['lib']\n\n[tool.uv.sources]\nlib = { path = 'link/../lib' }\n",
    )
    .unwrap();

    assert_failure_contains(pacquet_in(root.path()).arg("install"), "whose `..` follows the link");
}

/// A `..` after a directory that is not there is an error on POSIX and
/// nothing on Windows, so it is refused rather than read as either.
#[tokio::test]
async fn a_path_source_stepping_out_of_a_missing_directory_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    project(root.path(), &server.url(), &[]);
    fs::create_dir_all(root.path().join("lib")).unwrap();
    fs::write(
        root.path().join("lib/pyproject.toml"),
        "[project]\nname = 'lib'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n",
    )
    .unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['lib']\n\n[tool.uv.sources]\nlib = { path = 'absent/../lib' }\n",
    )
    .unwrap();

    assert_failure_contains(pacquet_in(root.path()).arg("install"), "which is not there");
}

/// A build backend is code from the index that a build runs, so it is
/// approved the way a dependency's build scripts are.
#[tokio::test]
async fn a_backend_nothing_approved_does_not_build_the_project() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        workspace.split_once("allowBuilds:").expect("the fixture approves builds").0,
    )
    .unwrap();
    python_project(root.path(), "app", "dependencies = []");

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "because the build requirement pkg:pypi/tinybackend is not approved to run",
    );
}

/// An install that only warns about a build it skipped leaves the project
/// out of its own environment, and says so.
#[tokio::test]
async fn an_unapproved_backend_warns_when_builds_are_not_strict() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _alpha = serve(&mut server, "alpha", &[("1.0", wheel("alpha", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!(
            "{}strictDepBuilds: false\n",
            workspace.split_once("allowBuilds:").expect("the fixture approves builds").0,
        ),
    )
    .unwrap();
    python_project(root.path(), "app", "dependencies = ['alpha>=1']");

    let output = pacquet_in(root.path())
        .arg("install")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("stdout:\n{stdout}\nstderr:\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    assert!(stdout.contains("is not approved to run"), "{stdout}");
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import app"])
        .assert()
        .failure();
}

/// A group reached only through another group's `include-group` still
/// names the projects it requires, so they come from the workspace.
#[tokio::test]
async fn a_workspace_project_an_included_group_requires_is_still_local() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    let _lib = serve(&mut server, "lib", &[("9.0", wheel("lib", "9.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    python_project(&root.path().join("packages/lib"), "lib", "dependencies = []");
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n\n[dependency-groups]\ndev = [{ include-group = 'test' }]\n\
         test = ['lib']\n\n[tool.uv.sources]\nlib = { workspace = true }\n",
    )
    .unwrap();

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    assert!(lock.contains(r#"path = "packages/lib""#), "takes the workspace project: {lock}");
    assert!(!lock.contains("9.0"), "never reaches the index for lib: {lock}");
}

/// A backend asks for what it needs once it can see the project, and what
/// it asks for runs in the build too.
#[tokio::test]
async fn a_requirement_the_backend_asks_for_is_approved_too() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let asking = format!(
        "{TINY_BACKEND}\ndef get_requires_for_build_editable(config_settings=None):\n    \
         return ['helper']\n",
    );
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", asking.as_str())]))],
    )
    .await;
    let _helper = serve(&mut server, "helper", &[("1.0", wheel("helper", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    python_project(root.path(), "app", "dependencies = []");

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "because the build requirement pkg:pypi/helper is not approved to run",
    );
}

#[tokio::test]
async fn a_workspace_project_as_a_build_requirement_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    python_project(&root.path().join("packages/tinybackend"), "tinybackend", "dependencies = []");
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n\n[build-system]\nrequires = ['tinybackend']\n\
         build-backend = 'tinybuild'\n",
    )
    .unwrap();

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "which is a project in its workspace",
    );
}

#[tokio::test]
async fn a_wheel_built_for_another_interpreter_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    // A wheel built for another interpreter, and consistent about it, so
    // what refuses it is the check against this interpreter's tags.
    let elsewhere = TINY_BACKEND.replace("py3-none-any", "py2-none-any");
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", elsewhere.as_str())]))],
    )
    .await;
    project(root.path(), &server.url(), &[]);
    python_project(root.path(), "app", "dependencies = []");

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "which this interpreter does not install",
    );
}

#[tokio::test]
async fn a_build_requirement_a_marker_excludes_is_not_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    python_project(&root.path().join("packages/elsewhere"), "elsewhere", "dependencies = []");
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n\n[build-system]\n\
         requires = ['tinybackend', \"elsewhere; sys_platform == 'nowhere'\"]\n\
         build-backend = 'tinybuild'\n",
    )
    .unwrap();
    fs::create_dir_all(root.path().join("src/app")).unwrap();
    fs::write(root.path().join("src/app/__init__.py"), "MARKER = 'built'\n").unwrap();

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import app; print(app.MARKER)"])
        .assert()
        .success()
        .stdout(if cfg!(windows) { "built\r\n" } else { "built\n" });
}

/// PEP 517's defaults are requirements the build runs like any other, so
/// a workspace that declares one of those names is as ambiguous.
#[tokio::test]
async fn a_default_build_requirement_naming_a_workspace_project_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    python_project(&root.path().join("packages/setuptools"), "setuptools", "dependencies = []");
    fs::create_dir_all(root.path().join("packages/legacy")).unwrap();
    fs::write(
        root.path().join("packages/legacy/pyproject.toml"),
        "[project]\nname = 'legacy'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = []\n",
    )
    .unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\n\
         dependencies = ['legacy']\n\n[tool.uv.sources]\nlegacy = { workspace = true }\n",
    )
    .unwrap();

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "which is a project in its workspace",
    );
}

#[tokio::test]
async fn a_backend_asking_for_a_workspace_project_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let asking = format!(
        "{TINY_BACKEND}\ndef get_requires_for_build_editable(config_settings=None):\n    \
         return ['elsewhere']\n",
    );
    let _backend = serve(
        &mut server,
        "tinybackend",
        &[("80.0", wheel("tinybackend", "80.0", "", &[("tinybuild.py", asking.as_str())]))],
    )
    .await;
    let _elsewhere =
        serve(&mut server, "elsewhere", &[("1.0", wheel("elsewhere", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &[]);
    python_project(&root.path().join("packages/elsewhere"), "elsewhere", "dependencies = []");
    python_project(root.path(), "app", "dependencies = []");

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "which is a project in its workspace",
    );
}

/// PEP 503 has one distribution name however it is spelled, and an
/// approval names a distribution.
#[tokio::test]
async fn an_allow_builds_key_names_the_distribution_however_it_is_written() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!(
            "{}allowBuilds:\n  pkg:pypi/TinyBackend: true\n",
            workspace.split_once("allowBuilds:").expect("the fixture approves builds").0,
        ),
    )
    .unwrap();
    python_project(root.path(), "app", "dependencies = []");

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();
    python(root.path())
        .args(["-c", "import app; print(app.MARKER)"])
        .assert()
        .success()
        .stdout(if cfg!(windows) { "workspace app\r\n" } else { "workspace app\n" });
}

/// npm and `PyPI` both publish `esbuild`, `ruff` and `black`, so approving
/// a build script must not approve a build backend nobody looked at.
#[tokio::test]
async fn an_allow_builds_key_naming_no_ecosystem_approves_no_python_build() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &[]);
    let workspace = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        format!(
            "{}allowBuilds:\n  tinybackend: true\n",
            workspace.split_once("allowBuilds:").expect("the fixture approves builds").0,
        ),
    )
    .unwrap();
    python_project(root.path(), "app", "dependencies = []");

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "pkg:pypi/tinybackend is not approved to run",
    );
}
