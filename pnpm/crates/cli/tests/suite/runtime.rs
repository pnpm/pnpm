use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
};

#[test]
fn runtime_unknown_subcommand_runs_with_default_ndjson_and_silent_reporters() {
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        fs::write(workspace.join("package.json"), "{}").expect("write package.json");

        let mut command = pacquet;
        if let Some(reporter) = reporter {
            command = command.with_arg(reporter);
        }
        command = command.with_arg("runtime").with_arg("unknown");
        let output = command.output().expect("spawn pacquet runtime");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "unknown runtime subcommand must fail");
        assert!(stderr.contains("ERR_PNPM_RUNTIME_UNKNOWN_SUBCOMMAND"), "stderr: {stderr}");

        drop(root);
    }
}

#[test]
fn setting_a_project_runtime_suggests_the_explicit_global_shim() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let version = "24.0.0-rc.4";
    let _mocks = crate::install_runtimes::mock_node_release(&mut server, version);
    let pnpm_home = root.path().join("pnpm-home");
    configure_node_runtime(&root, &workspace, &server);

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_args(["runtime", "set", "node", version])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("pnpm shim add node"), "stdout was:\n{stdout}");
    assert!(
        !pnpm_home
            .join("bin")
            .join(format!("node{}", std::env::consts::EXE_SUFFIX))
            .exists(),
    );
}

#[test]
fn unset_deletes_a_matching_global_runtime_group() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    let global_pkg_dir = pnpm_home.join("global/v11");
    let global_bin = pnpm_home.join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let (link, install_dir) = seed_global_runtime(&global_pkg_dir, "node", "22.14.0", "hash-node");
    fs::write(global_bin.join("node"), "shim").unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "node", "22.14.0", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr={stderr}");
    assert!(!link.exists(), "global hash link should be removed");
    assert!(!install_dir.exists(), "global install dir should be removed");
    assert!(!global_bin.join("node").exists(), "global bin should be removed");
}

#[test]
fn unset_without_a_global_bin_dir_deletes_the_store_group() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    let global_pkg_dir = pnpm_home.join("global/v11");
    let (link, install_dir) = seed_global_runtime(&global_pkg_dir, "node", "20.19.0", "hash-node");

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "node", "20.19", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr={stderr}");
    assert!(!link.exists());
    assert!(!install_dir.exists());
}

#[test]
fn unset_keeps_other_versions_of_the_same_runtime() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    let global_pkg_dir = pnpm_home.join("global/v11");
    fs::create_dir_all(pnpm_home.join("bin")).unwrap();
    let (removed_link, removed_dir) =
        seed_global_runtime(&global_pkg_dir, "deno", "2.1.0", "hash-deno-2");
    let (kept_link, kept_dir) =
        seed_global_runtime(&global_pkg_dir, "deno", "1.46.0", "hash-deno-1");

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "deno", "2.1", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr={stderr}");
    assert!(!removed_link.exists());
    assert!(!removed_dir.exists());
    assert!(kept_link.exists());
    assert!(kept_dir.exists());
}

#[test]
fn unset_without_a_version_deletes_every_installed_version() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    let global_pkg_dir = pnpm_home.join("global/v11");
    fs::create_dir_all(pnpm_home.join("bin")).unwrap();
    let (first_link, first_dir) =
        seed_global_runtime(&global_pkg_dir, "bun", "1.1.0", "hash-bun-1");
    let (second_link, second_dir) =
        seed_global_runtime(&global_pkg_dir, "bun", "1.2.0", "hash-bun-2");

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "rm", "bun", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr={stderr}");
    assert!(!first_link.exists());
    assert!(!first_dir.exists());
    assert!(!second_link.exists());
    assert!(!second_dir.exists());
}

#[test]
fn unset_cleans_up_the_active_legacy_node_link() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    let legacy_version_dir = pnpm_home.join("nodejs/22.14.0");
    fs::create_dir_all(&legacy_version_dir).unwrap();
    let active_link = pnpm_home.join("nodejs_current");
    pnpm_fs::symlink_dir(&legacy_version_dir, &active_link).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "node", "22.14", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr={stderr}");
    assert!(!legacy_version_dir.exists());
    assert!(!active_link.exists());
}

#[test]
fn unset_fails_when_the_runtime_is_not_installed() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(pnpm_home.join("bin")).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "node", "22", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_GLOBAL_PKG_NOT_FOUND"), "stderr={stderr}");
}

#[test]
fn unset_rejects_an_unsupported_runtime_name() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();

    let output = pacquet
        .with_args(["runtime", "unset", "python", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_INVALID_RUNTIME_NAME"), "stderr={stderr}");
    drop(root);
    drop(workspace);
}

fn seed_global_runtime(
    global_pkg_dir: &Path,
    runtime_name: &str,
    version: &str,
    hash: &str,
) -> (PathBuf, PathBuf) {
    let install_dir = global_pkg_dir.join(format!("install-{hash}"));
    let runtime_dir = install_dir.join("node_modules").join(runtime_name);
    fs::create_dir_all(runtime_dir.join("bin")).unwrap();
    fs::write(
        install_dir.join("package.json"),
        json!({ "dependencies": { runtime_name: format!("runtime:{version}") } }).to_string(),
    )
    .unwrap();
    fs::write(
        runtime_dir.join("package.json"),
        json!({
            "name": runtime_name,
            "version": version,
            "bin": { runtime_name: format!("bin/{runtime_name}") },
        })
        .to_string(),
    )
    .unwrap();
    fs::write(runtime_dir.join("bin").join(runtime_name), "runtime").unwrap();
    let link = global_pkg_dir.join(hash);
    pnpm_fs::symlink_dir(&install_dir, &link).unwrap();
    (link, install_dir)
}

fn configure_node_runtime(root: &tempfile::TempDir, workspace: &Path, server: &mockito::Server) {
    let config_dir = root.path().join("config").join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.yaml"),
        format!(
            "storeDir: {}\ncacheDir: {}\nnodeDownloadMirrors:\n  rc: '{}/'\n",
            root.path().join("store").display(),
            root.path().join("cache").display(),
            server.url(),
        ),
    )
    .unwrap();
    fs::write(workspace.join("package.json"), "{}").unwrap();
}
