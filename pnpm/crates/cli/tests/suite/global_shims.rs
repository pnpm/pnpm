//! End-to-end tests for context-aware global shim dispatch
//! (`pnpm --shim <name> <target> -- <args>`).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const AUTO_TRUST_ENV: &str = "PNPM_AUTO_APPROVE_PROJECT_BINS_FOR_TESTS";

/// A `pnpm --shim` invocation with an isolated pnpm home and state dir so
/// the dispatcher can neither see the developer's global installs nor
/// their trust registry.
fn shim_command(root: &TempDir, cwd: &Path, shim_args: &[&str]) -> Command {
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(cwd)
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
        .with_arg("--shim")
        .with_args(shim_args)
}

#[cfg(unix)]
fn write_script(path: &Path, output: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\necho {output}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A project dir whose `node_modules/.bin/<name>` links to a script
/// provided by the package `provider` (printing `local`), plus a global
/// target provided by a package named `name` (printing `global`).
/// Returns `(project_dir, global_target)`.
#[cfg(unix)]
fn prepare_local_and_global_from(
    root: &TempDir,
    name: &str,
    provider: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let project = root.path().join("project");
    write_script(&project.join("node_modules").join(provider).join("cli.sh"), "local");
    let bin = project.join("node_modules").join(".bin").join(name);
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(format!("../{provider}/cli.sh"), &bin).unwrap();
    let global_target = root.path().join("global").join("node_modules").join(name).join("cli.sh");
    write_script(&global_target, "global");
    (project, global_target)
}

#[cfg(unix)]
fn prepare_local_and_global(
    root: &TempDir,
    name: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    prepare_local_and_global_from(root, name, name)
}

#[cfg(unix)]
#[test]
fn trusted_project_local_bin_wins() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "local");
}

#[cfg(unix)]
#[test]
fn untrusted_project_falls_back_to_the_global_target() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    // No recorded decision and no terminal to ask on: the global target
    // must run.
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

#[cfg(unix)]
#[test]
fn recorded_trust_decision_is_honored() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    // The dispatcher keys the registry by the path it observes as the
    // process cwd, which the OS reports symlink-resolved.
    let project_key = fs::canonicalize(&project).unwrap();
    let trust_file = root.path().join("state").join("pnpm").join("global-bin-trust.jsonl");
    fs::create_dir_all(trust_file.parent().unwrap()).unwrap();

    for (allow, expected) in [(true, "local"), (false, "global")] {
        fs::write(
            &trust_file,
            format!(
                "{}\n",
                serde_json::json!({ "projectDir": project_key, "allow": allow, "decidedAt": 0 }),
            ),
        )
        .unwrap();
        let output =
            shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
                .assert()
                .success();
        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert_eq!(stdout.trim(), expected, "allow={allow}");
    }
}

#[cfg(unix)]
#[test]
fn bypass_env_skips_the_project_bin() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_SHIM_BYPASS", "1")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

#[cfg(unix)]
#[test]
fn shim_args_reach_the_target() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let script = project.join("node_modules").join("tool").join("cli.sh");
    fs::write(&script, "#!/bin/sh\necho \"$@\"\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let output = shim_command(
        &root,
        &project,
        &["tool", global_target.to_str().unwrap(), "--", "--flag", "value with spaces"],
    )
    .with_env(AUTO_TRUST_ENV, "1")
    .assert()
    .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "--flag value with spaces");
}

#[cfg(unix)]
#[test]
fn missing_global_target_reports_not_found() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("plain");
    fs::create_dir_all(&cwd).unwrap();
    let output = shim_command(&root, &cwd, &["tool", "/nonexistent/tool", "--"]).assert().failure();
    assert_eq!(output.get_output().status.code(), Some(127));
}

/// A project that pins node in `devEngines.runtime` but has no
/// `node_modules` gets the pinned version fetched on demand (through the
/// dlx machinery) instead of the global target.
#[cfg(unix)]
#[test]
fn runtime_pin_downloads_node_on_demand() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new();
    let version = "24.0.0-rc.4";
    let _mocks = crate::install_runtimes::mock_node_release(&mut server, version);

    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: {}\ncacheDir: {}\nenableGlobalVirtualStore: false\nnodeDownloadMirrors:\n  rc: '{}/'\n",
            root.path().join("store").display(),
            root.path().join("cache").display(),
            server.url(),
        ),
    )
    .unwrap();
    fs::write(
        project.join("package.json"),
        serde_json::json!({
            "devEngines": { "runtime": { "name": "node", "version": version } },
        })
        .to_string(),
    )
    .unwrap();
    let global_target =
        root.path().join("global").join("node_modules").join("node").join("bin").join("node");
    write_script(&global_target, "global");

    shim_command(&root, &project, &["node", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .assert()
        .success();
    // The only source of a node binary in this test is the mocked
    // release server, so a materialized dlx slot proves the pinned
    // version was fetched and run (the fixture binary exits 0).
    let dlx_cache = root.path().join("cache").join("dlx");
    let materialized_node = fs::read_dir(&dlx_cache)
        .expect("the dlx cache should exist")
        .flatten()
        .any(|entry| entry.path().join("pkg").join("node_modules").join("node").is_dir());
    assert!(materialized_node, "the pinned node should be materialized under {dlx_cache:?}");
}

/// A same-named bin provided by a *different* package must not shadow
/// the global one, trusted or not.
#[cfg(unix)]
#[test]
fn lookalike_package_does_not_shadow_the_global_bin() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global_from(&root, "tool", "evil-pkg");
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

/// `globalShims: false` in the global config.yaml disables dispatch
/// immediately — no relinking required.
#[cfg(unix)]
#[test]
fn global_shims_setting_off_disables_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let config_dir = root.path().join("config").join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), "globalShims: false\n").unwrap();
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

/// The `PNPM_CONFIG_GLOBAL_SHIMS` env override wins over the config file.
#[cfg(unix)]
#[test]
fn global_shims_env_override_disables_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "false")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

/// A project whose lockfile was verified by an install on this machine
/// runs its local bins without any prompt or recorded trust decision.
#[cfg(unix)]
#[test]
fn locally_verified_lockfile_skips_the_trust_gate() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    write_lockfile_verified_record(&root, &project);
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "local");
}

/// A verification record that no longer matches the lockfile's stat is
/// not proof — the dispatcher falls back to the (unanswerable) prompt
/// and the global target runs.
#[cfg(unix)]
#[test]
fn stale_lockfile_verification_record_does_not_skip_the_trust_gate() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    write_lockfile_verified_record(&root, &project);
    fs::write(project.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n# edited\n").unwrap();
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

/// Write a lockfile into `project` plus a matching machine-local
/// verification record, the way an install's verification gate would.
#[cfg(unix)]
fn write_lockfile_verified_record(root: &TempDir, project: &Path) {
    use std::os::unix::fs::MetadataExt;
    let lockfile = project.join("pnpm-lock.yaml");
    fs::write(&lockfile, "lockfileVersion: '9.0'\n").unwrap();
    let metadata = fs::metadata(&lockfile).unwrap();
    let mtime_ns = metadata
        .modified()
        .unwrap()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string();
    let record = serde_json::json!({
        "lockfile": {
            "hash": "0000",
            "path": fs::canonicalize(&lockfile).unwrap(),
            "size": metadata.len(),
            "mtimeNs": mtime_ns,
            "inode": metadata.ino().to_string(),
        },
        "verifiedAt": "2026-01-01T00:00:00.000Z",
        "policy": { "tarballUrlBinding": true, "integrityRequired": true },
    });
    let cache_dir = root.path().join("cache-home").join("pnpm");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("lockfile-verified.jsonl"), format!("{record}\n")).unwrap();
}

#[test]
fn malformed_shim_invocation_errors() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("plain");
    fs::create_dir_all(&cwd).unwrap();
    let output = shim_command(&root, &cwd, &["tool"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("malformed --shim invocation"), "stderr was:\n{stderr}");
}
