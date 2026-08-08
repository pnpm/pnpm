//! End-to-end tests for context-aware global shim dispatch
//! (`pnpm --shim <name> <shim> <target> -- <args>`).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const AUTO_TRUST_ENV: &str = "PNPM_AUTO_APPROVE_PROJECT_BINS_FOR_TESTS";

/// A `pnpm --shim` invocation with an isolated pnpm home and state dir so
/// the dispatcher can neither see the developer's global installs nor
/// their trust registry.
fn shim_command(root: &TempDir, cwd: &Path, shim_args: &[&str]) -> Command {
    let mut command = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(cwd)
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
        .with_arg("--shim");
    if let Some((name, tail)) = shim_args.split_first() {
        command = command
            .with_arg(name)
            .with_arg(root.path().join("nonexistent-generated-shim"))
            .with_args(tail);
    }
    command
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
    fs::write(
        project.join("node_modules").join(provider).join("package.json"),
        serde_json::json!({ "name": provider, "version": "1.0.0" }).to_string(),
    )
    .unwrap();
    let bin = project.join("node_modules").join(".bin").join(name);
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(format!("../{provider}/cli.sh"), &bin).unwrap();
    let global_target = root.path().join("global").join("node_modules").join(name).join("cli.sh");
    write_script(&global_target, "global");
    fs::write(
        global_target.parent().unwrap().join("package.json"),
        serde_json::json!({ "name": name, "version": "1.0.0" }).to_string(),
    )
    .unwrap();
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
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "all")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "local");
}

#[cfg(unix)]
#[test]
fn ordinary_project_bins_do_not_switch_in_auto_mode() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

#[cfg(unix)]
#[test]
fn untrusted_project_falls_back_to_the_global_target() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    // No recorded decision and no terminal to ask on: the global target
    // must run.
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "all")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

#[cfg(unix)]
#[test]
fn bypass_env_skips_the_project_bin() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "all")
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
    .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "all")
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

/// A project that pins Node.js gets the pinned version fetched into the
/// global virtual store instead of using the project `.bin` or global target.
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
    write_script(&project.join("node_modules/.bin/node"), "compromised-local-bin");
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
    fs::write(
        global_target.parent().unwrap().parent().unwrap().join("package.json"),
        serde_json::json!({ "name": "node", "version": "1.0.0" }).to_string(),
    )
    .unwrap();

    shim_command(&root, &project, &["node", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "all")
        .assert()
        .success();
    let environments = root.path().join("state/pnpm/global-shim-runtimes");
    let package_dir = fs::read_dir(&environments)
        .expect("the runtime environment should exist")
        .flatten()
        .map(|entry| entry.path().join("node_modules/node"))
        .find(|path| path.exists())
        .expect("the runtime environment should link Node.js");
    let package_dir = fs::canonicalize(package_dir).unwrap();
    let global_store = fs::canonicalize(root.path().join("store/v11/links")).unwrap();
    assert!(package_dir.starts_with(global_store));
    assert!(!root.path().join("cache/dlx").exists());
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
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "all")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

/// `globalShims: off` in the global config.yaml disables dispatch
/// immediately — no relinking required.
#[cfg(unix)]
#[test]
fn global_shims_setting_off_disables_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let config_dir = root.path().join("config").join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), "globalShims: off\n").unwrap();
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

#[cfg(unix)]
#[test]
fn global_home_setting_off_disables_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(&pnpm_home).unwrap();
    fs::write(pnpm_home.join("pnpm-workspace.yaml"), "globalShims: off\n").unwrap();
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
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "off")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

/// A lockfile-verification cache record proves dependency resolution integrity,
/// not authorization to replace a global command. Without an explicit trust
/// decision, a non-interactive invocation must still use the global target.
#[cfg(unix)]
#[test]
fn verified_lockfile_does_not_skip_the_trust_gate() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    write_lockfile_verified_record(&root, &project);
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "all")
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

#[cfg(unix)]
#[test]
fn global_fallback_preserves_quoted_shebang_arguments() {
    use pacquet_cmd_shim::{Host, ShimStyle, generate_sh_shim, search_script_runtime};
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let global_bin = root.path().join("global-bin");
    let target = root.path().join("global/node_modules/tool/cli");
    let interpreter = root.path().join("capture-argv");
    let shim = global_bin.join("tool");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::create_dir_all(&global_bin).unwrap();
    fs::write(&interpreter, "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\n").unwrap();
    fs::set_permissions(&interpreter, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&target, format!("#!{} --label \"value with spaces\"\n", interpreter.display()))
        .unwrap();
    let runtime = search_script_runtime::<Host>(&target).unwrap();
    fs::write(
        &shim,
        generate_sh_shim(&target, &shim, runtime.as_ref(), &[], ShimStyle::ContextAware),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    fs::hard_link(assert_cmd::cargo::cargo_bin("pnpm"), global_bin.join(".pnpm-shim-v1"))
        .or_else(|_| {
            fs::copy(assert_cmd::cargo::cargo_bin("pnpm"), global_bin.join(".pnpm-shim-v1"))
                .map(|_| ())
        })
        .unwrap();
    let cwd = root.path().join("outside");
    fs::create_dir_all(&cwd).unwrap();

    let output = Command::new(&shim)
        .with_current_dir(&cwd)
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .arg("forwarded with spaces")
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let lines: Vec<_> =
        String::from_utf8_lossy(&output.stdout).lines().map(str::to_string).collect();
    assert_eq!(lines.first().map(String::as_str), Some("<--label>"));
    assert_eq!(lines.get(1).map(String::as_str), Some("<value with spaces>"));
    assert_eq!(lines.last().map(String::as_str), Some("<forwarded with spaces>"));
}

#[cfg(windows)]
#[test]
fn generated_cmd_and_powershell_shims_dispatch_and_fall_back() {
    use pacquet_cmd_shim::{ShimStyle, generate_cmd_shim, generate_pwsh_shim};

    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let outside = root.path().join("outside");
    let local_package = project.join("node_modules/tool");
    let local_target = local_package.join("cli.cmd");
    let local_bin = project.join("node_modules/.bin");
    let global_package = root.path().join("global/node_modules/tool");
    let global_target = global_package.join("cli.cmd");
    let global_bin = root.path().join("global-bin");
    fs::create_dir_all(&local_package).unwrap();
    fs::create_dir_all(&local_bin).unwrap();
    fs::create_dir_all(&global_package).unwrap();
    fs::create_dir_all(&global_bin).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(local_package.join("package.json"), r#"{"name":"tool"}"#).unwrap();
    fs::write(global_package.join("package.json"), r#"{"name":"tool"}"#).unwrap();
    fs::write(&local_target, "@ECHO local:%*\r\n").unwrap();
    fs::write(&global_target, "@ECHO global:%*\r\n").unwrap();
    fs::write(local_bin.join("tool"), format!("# cmd-shim-target={}\n", local_target.display()))
        .unwrap();
    fs::write(local_bin.join("tool.cmd"), format!("@CALL \"{}\" %*\r\n", local_target.display()))
        .unwrap();
    let cmd_shim = global_bin.join("tool.cmd");
    let pwsh_shim = global_bin.join("tool.ps1");
    fs::write(
        &cmd_shim,
        generate_cmd_shim(&global_target, &cmd_shim, None, &[], ShimStyle::ContextAware),
    )
    .unwrap();
    fs::write(
        &pwsh_shim,
        generate_pwsh_shim(&global_target, &pwsh_shim, None, &[], ShimStyle::ContextAware),
    )
    .unwrap();
    fs::hard_link(assert_cmd::cargo::cargo_bin("pnpm"), global_bin.join(".pnpm-shim-v1.exe"))
        .or_else(|_| {
            fs::copy(assert_cmd::cargo::cargo_bin("pnpm"), global_bin.join(".pnpm-shim-v1.exe"))
                .map(|_| ())
        })
        .unwrap();

    for (shell, shell_args, shim) in [
        ("cmd", vec!["/c"], &cmd_shim),
        ("powershell.exe", vec!["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"], &pwsh_shim),
    ] {
        let local = Command::new(shell)
            .args(&shell_args)
            .arg(shim)
            .arg("value with spaces")
            .current_dir(&project)
            .env(AUTO_TRUST_ENV, "1")
            .env("PNPM_CONFIG_GLOBAL_SHIMS", "all")
            .env("PNPM_HOME", root.path().join("pnpm-home"))
            .env("XDG_STATE_HOME", root.path().join("state"))
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .output()
            .unwrap();
        assert!(local.status.success(), "stderr:\n{}", String::from_utf8_lossy(&local.stderr));
        let local_stdout = String::from_utf8_lossy(&local.stdout);
        assert!(local_stdout.contains("local:"));
        assert!(local_stdout.contains("value with spaces"));

        let global = Command::new(shell)
            .args(&shell_args)
            .arg(shim)
            .arg("value with spaces")
            .current_dir(&outside)
            .env("PNPM_HOME", root.path().join("pnpm-home"))
            .env("XDG_STATE_HOME", root.path().join("state"))
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .output()
            .unwrap();
        assert!(global.status.success(), "stderr:\n{}", String::from_utf8_lossy(&global.stderr));
        let global_stdout = String::from_utf8_lossy(&global.stdout);
        assert!(global_stdout.contains("global:"));
        assert!(global_stdout.contains("value with spaces"));
    }
}

#[cfg(windows)]
#[test]
fn native_node_dispatcher_preserves_the_global_executable_fallback() {
    use std::os::windows::ffi::OsStrExt as _;

    let root = tempfile::tempdir().unwrap();
    let global_bin = root.path().join("global-bin");
    let outside = root.path().join("outside");
    fs::create_dir_all(&global_bin).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::copy(Command::cargo_bin("pnpm").unwrap().get_program(), global_bin.join("node.exe"))
        .unwrap();

    let global_target = std::env::var_os("ComSpec").expect("ComSpec should identify cmd.exe");
    let encoded = global_target.encode_wide().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    fs::write(global_bin.join(".pnpm-shim-v1-node-target"), encoded).unwrap();

    let output = Command::new(global_bin.join("node.exe"))
        .args(["/d", "/c", "echo native-fallback"])
        .current_dir(outside)
        .env("PNPM_CONFIG_GLOBAL_SHIMS", "auto")
        .env("PNPM_HOME", root.path().join("pnpm-home"))
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("native-fallback"));
}
