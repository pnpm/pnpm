use super::{
    Cursor, SimpleFileOptions, Write, ZipWriter, assert_failure_contains, cargo_project,
    flatten_report, fs, json, pacquet_in, project, python, serve, wheel, wheel_with_tags,
};
use assert_cmd::assert::OutputAssertExt;

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

#[test]
fn broken_python_environment_errors_identify_the_missing_path() {
    for missing_target in [false, true] {
        eprintln!("missing_target={missing_target}");
        let root = tempfile::tempdir().unwrap();
        let project_root = dunce::canonicalize(root.path()).unwrap();
        project(&project_root, "https://unused.invalid", &[]);
        let target = if missing_target {
            project_root.join(".pnpm").join("python-envs").join("env-missing")
        } else {
            project_root.join("unmanaged")
        };
        fs::create_dir_all(&target).unwrap();
        pnpm_fs::force_symlink_dir(&target, &project_root.join(".venv")).unwrap();
        if missing_target {
            fs::remove_dir(&target).unwrap();
        }
        let missing = if missing_target { target } else { project_root.join(".pnpm/python-envs") };
        assert_failure_contains(
            pacquet_in(root.path()).args(["install", "--offline"]),
            &format!("{} for {}", missing.display(), project_root.join(".venv").display()),
        );
        assert!(!root.path().join("pylock.toml").exists(), "published failed environment metadata");
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
fn python_add_rejects_dynamic_metadata_without_mutating_the_manifest() {
    for development in [false, true] {
        let root = tempfile::tempdir().unwrap();
        project(root.path(), "https://unused.invalid", &[]);
        let manifest = "[project]\nname = 'dynamic-app'\ndynamic = ['dependencies']\n";
        fs::write(root.path().join("pyproject.toml"), manifest).unwrap();
        let mut command = pacquet_in(root.path());
        command.args(["add", "pypi:alpha"]);
        if development {
            command.arg("--save-dev");
        }
        assert_failure_contains(&mut command, "requires static dependency metadata");
        let actual = fs::read_to_string(root.path().join("pyproject.toml")).unwrap();
        eprintln!("manifest after rejection:\n{actual}");
        assert_eq!(actual, manifest);
        assert!(!root.path().join("pylock.toml").exists());
    }
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
