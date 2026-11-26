use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils_registry::bin::CommandTempCwd;
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
fn unset_removes_a_caret_declared_runtime_by_major_version() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({ "devDependencies": { "node": "runtime:^24.0.0-rc.4" } }).to_string();
    fs::write(workspace.join("package.json"), &manifest).unwrap();

    let output = pacquet
        .with_args(["runtime", "unset", "node", "24"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr={stderr}");

    let manifest = fs::read_to_string(workspace.join("package.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert!(manifest["devDependencies"].get("node").is_none(), "manifest: {manifest}");
    drop(root);
}

#[test]
fn unset_without_a_flag_removes_an_engines_runtime() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "engines": { "runtime": { "name": "node", "version": "^24.0.0-rc.4", "onFail": "download" } },
    })
    .to_string();
    fs::write(workspace.join("package.json"), &manifest).unwrap();

    let output = pacquet
        .with_args(["runtime", "unset", "node"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr={stderr}");

    let manifest = fs::read_to_string(workspace.join("package.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert!(manifest["engines"].get("runtime").is_none(), "manifest: {manifest}");
    drop(root);
}

#[test]
fn unset_without_a_flag_removes_the_runtime_from_both_engines_fields() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let runtime = json!({ "name": "node", "version": "^24.0.0-rc.4", "onFail": "download" });
    let manifest =
        json!({ "engines": { "runtime": runtime }, "devEngines": { "runtime": runtime } })
            .to_string();
    fs::write(workspace.join("package.json"), &manifest).unwrap();

    let output = pacquet
        .with_args(["runtime", "unset", "node"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr={stderr}");

    let manifest = fs::read_to_string(workspace.join("package.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert!(manifest["engines"].get("runtime").is_none(), "manifest: {manifest}");
    assert!(manifest["devEngines"].get("runtime").is_none(), "manifest: {manifest}");
    drop(root);
}

#[test]
fn unset_rejects_a_blank_version_before_touching_global_runtimes() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["runtime", "unset", "node", " ", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_INVALID_RUNTIME_VERSION"), "stderr={stderr}");
    drop(root);
}

#[test]
fn unset_leaves_an_ordinary_project_dependency_named_like_a_runtime() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({ "devDependencies": { "node": "^22.0.0" } }).to_string();
    fs::write(workspace.join("package.json"), &manifest).unwrap();

    let output = pacquet
        .with_args(["runtime", "unset", "node"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_RUNTIME_NOT_DECLARED"), "stderr={stderr}");
    assert_eq!(fs::read_to_string(workspace.join("package.json")).unwrap(), manifest);
    drop(root);
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
        .with_args(["runtime", "unset", "bun", "-g"])
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

#[cfg(unix)]
#[test]
fn unset_preserves_an_unrelated_dangling_npm_link_during_legacy_cleanup() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    let legacy_version_dir = pnpm_home.join("nodejs/22.14.0");
    fs::create_dir_all(&legacy_version_dir).unwrap();
    let global_bin = pnpm_home.join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let node_link = global_bin.join("node");
    std::os::unix::fs::symlink(legacy_version_dir.join("bin/node"), &node_link).unwrap();
    let npm_link = global_bin.join("npm");
    let unrelated_target = root.path().join("other-global-install/npm/bin/npm");
    std::os::unix::fs::symlink(&unrelated_target, &npm_link).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "node", "22.14", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr={stderr}");
    assert!(!legacy_version_dir.exists());
    assert!(node_link.symlink_metadata().is_err());
    assert!(
        npm_link
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink(),
    );
    assert_eq!(fs::read_link(npm_link).unwrap(), unrelated_target);
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
fn unset_leaves_an_ordinary_package_named_like_a_runtime() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    let global_pkg_dir = pnpm_home.join("global/v11");
    let global_bin = pnpm_home.join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let (link, install_dir) =
        seed_global_package(&global_pkg_dir, "bun", "1.2.0", "hash-bun", "^1.2.0");
    fs::write(global_bin.join("bun"), "shim").unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "bun", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_GLOBAL_PKG_NOT_FOUND"), "stderr={stderr}");
    assert!(link.exists());
    assert!(install_dir.exists());
    assert!(global_bin.join("bun").exists());
}

#[cfg(unix)]
#[test]
fn unset_does_not_remove_an_npm_shim_under_a_version_named_home_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("22.14.0/pnpm-home");
    let global_pkg_dir = pnpm_home.join("global/v11");
    let global_bin = pnpm_home.join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let (node_link, node_install_dir) =
        seed_global_runtime(&global_pkg_dir, "node", "22.14.0", "hash-node");
    let (npm_link, npm_install_dir) =
        seed_global_package(&global_pkg_dir, "npm", "10.0.0", "hash-npm", "^10.0.0");
    let npm_bin = global_bin.join("npm");
    let npm_target = npm_install_dir.join("node_modules/npm/bin/npm");
    std::os::unix::fs::symlink(&npm_target, &npm_bin).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "node", "22.14.0", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr={stderr}");
    assert!(!node_link.exists());
    assert!(!node_install_dir.exists());
    assert!(npm_link.exists());
    assert!(npm_install_dir.exists());
    assert!(
        npm_bin
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink(),
    );
    assert_eq!(fs::read_link(npm_bin).unwrap(), npm_target);
}

#[cfg(unix)]
#[test]
fn unset_keeps_an_unrelated_stale_node_link_when_removing_another_runtime() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(&pnpm_home).unwrap();
    let current = pnpm_home.join("nodejs_current");
    std::os::unix::fs::symlink(pnpm_home.join("nodejs/18.12.1/bin/node"), &current).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "bun", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_GLOBAL_PKG_NOT_FOUND"), "stderr={stderr}");
    assert!(current.symlink_metadata().is_ok());
}

#[cfg(unix)]
#[test]
fn unset_fails_when_only_a_stale_global_bin_link_exists() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let stale_link = global_bin.join("bun");
    std::os::unix::fs::symlink(root.path().join("missing-bun-install"), &stale_link).unwrap();

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_args(["runtime", "unset", "bun", "-g"])
        .output()
        .expect("spawn pacquet runtime unset");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_GLOBAL_PKG_NOT_FOUND"), "stderr={stderr}");
    assert!(stale_link.symlink_metadata().is_ok());
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
    seed_global_package(global_pkg_dir, runtime_name, version, hash, &format!("runtime:{version}"))
}

fn seed_global_package(
    global_pkg_dir: &Path,
    package_name: &str,
    version: &str,
    hash: &str,
    specifier: &str,
) -> (PathBuf, PathBuf) {
    let install_dir = global_pkg_dir.join(format!("install-{hash}"));
    let package_dir = install_dir.join("node_modules").join(package_name);
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(
        install_dir.join("package.json"),
        json!({ "dependencies": { package_name: specifier } }).to_string(),
    )
    .unwrap();
    fs::write(
        package_dir.join("package.json"),
        json!({
            "name": package_name,
            "version": version,
            "bin": { package_name: format!("bin/{package_name}") },
        })
        .to_string(),
    )
    .unwrap();
    fs::write(package_dir.join("bin").join(package_name), "runtime").unwrap();
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
            "storeDir: {}\ncacheDir: {}\nnodeDownloadMirrors:\n  release: '{}/'\n  rc: '{}/'\n",
            root.path().join("store").display(),
            root.path().join("cache").display(),
            server.url(),
            server.url(),
        ),
    )
    .unwrap();
    fs::write(workspace.join("package.json"), "{}").unwrap();
}
