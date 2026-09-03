//! End-to-end tests for context-aware global shim dispatch: the pnpm
//! executable launched under a bin name, with the global target recorded
//! beside it.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const AUTO_TRUST_ENV: &str = "PNPM_AUTO_APPROVE_PROJECT_BINS_FOR_TESTS";

/// Install the shim `name` for `target` into an isolated global bin dir
/// under `root`, as a global install would, and return a command that
/// launches it from `cwd` with an isolated pnpm home and state dir, so the
/// dispatcher can neither see the developer's global installs nor their
/// trust registry.
#[cfg(unix)]
fn shim_command(root: &TempDir, cwd: &Path, name: &str, target: &str) -> Command {
    let shim = install_shim(&root.path().join("global-bin"), name, target.as_bytes());
    Command::new(shim)
        .without_ambient_pnpm_config()
        .with_current_dir(cwd)
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
}

/// Publish the pnpm executable under test as the shim `name` in
/// `global_bin`, recording the encoded `target` beside it.
fn install_shim(global_bin: &Path, name: &str, target: &[u8]) -> std::path::PathBuf {
    let shim = global_bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    fs::create_dir_all(global_bin).unwrap();
    // A slot holding an earlier hard link of the executable must be cleared
    // rather than copied over: the copy would truncate the executable itself.
    match fs::remove_file(&shim) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("clear the shim slot at {}: {error}", shim.display()),
    }
    let executable = assert_cmd::cargo::cargo_bin("pnpm");
    fs::hard_link(&executable, &shim)
        .or_else(|_| fs::copy(&executable, &shim).map(|_| ()))
        .unwrap();
    fs::write(global_bin.join(format!(".pnpm-shim-v1-{name}-target")), target).unwrap();
    shim
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
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "local");
}

#[cfg(unix)]
#[test]
fn unlisted_packages_do_not_switch_by_default() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
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
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
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
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
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
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
        .with_args(["--flag", "value with spaces"])
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
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
    let output = shim_command(&root, &cwd, "tool", "/nonexistent/tool").assert().failure();
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
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    // The project's own configuration must not reach the runtime install:
    // a repo-controlled store or mirror could feed the dispatcher a
    // poisoned artifact. Both entries here would fail the test if honored.
    fs::write(
        project.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: {}\nnodeDownloadMirrors:\n  rc: 'http://127.0.0.1:1/'\n",
            root.path().join("evil-store").display(),
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

    shim_command(&root, &project, "node", global_target.to_str().unwrap())
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
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
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
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
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
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
    fs::write(pnpm_home.join("pnpm-workspace.yaml"), "globalShims: false\n").unwrap();
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
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
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
        .with_env(AUTO_TRUST_ENV, "1")
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "false")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

#[cfg(unix)]
#[test]
fn malformed_trusted_settings_disable_dispatch() {
    for source in ["global config", "pnpm home", "environment"] {
        let root = tempfile::tempdir().unwrap();
        let (project, global_target) = prepare_local_and_global(&root, "tool");
        let config_dir = root.path().join("config").join("pnpm");
        let pnpm_home = root.path().join("pnpm-home");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&pnpm_home).unwrap();

        let mut command = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
            .with_env(AUTO_TRUST_ENV, "1");
        match source {
            "global config" => {
                fs::write(config_dir.join("config.yaml"), "globalShims: [\n").unwrap();
                command = command.with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#);
            }
            "pnpm home" => {
                fs::write(pnpm_home.join("pnpm-workspace.yaml"), "globalShims: [\n").unwrap();
                command = command.with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#);
            }
            "environment" => {
                fs::write(config_dir.join("config.yaml"), "globalShims: {tool: true}\n").unwrap();
                command = command.with_env("PNPM_CONFIG_GLOBAL_SHIMS", "[");
            }
            _ => unreachable!(),
        }

        let output = command.assert().success();
        let stdout = String::from_utf8_lossy(&output.get_output().stdout);
        assert_eq!(stdout.trim(), "global", "source: {source}");
        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        assert!(
            stderr.contains("project-aware global shims are disabled"),
            "source: {source}\nstderr:\n{stderr}",
        );
    }
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
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
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

/// A `pnpm-workspace.yaml` in an ancestor of the pnpm home must not
/// influence dispatch — only `<home>/pnpm-workspace.yaml` itself may.
#[cfg(unix)]
#[test]
fn ancestors_of_the_pnpm_home_cannot_set_the_mode() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    // `PNPM_HOME` points at `<root>/pnpm-home`; its parent tries to
    // enable dispatch for `tool`. Without env or home-yaml entries the
    // defaults must apply, under which an ordinary tool never switches.
    fs::write(root.path().join("pnpm-workspace.yaml"), "globalShims: {tool: true}\n").unwrap();
    fs::create_dir_all(root.path().join("pnpm-home")).unwrap();
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
        .with_env(AUTO_TRUST_ENV, "1")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

/// `"always"` pre-answers the prompt: an unsigned candidate switches
/// even where no terminal could ask.
#[cfg(unix)]
#[test]
fn always_policy_switches_without_a_prompt() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let output = shim_command(&root, &project, "tool", global_target.to_str().unwrap())
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": "always"}"#)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "local");
}

/// The dispatcher runs a script target under its shebang interpreter the
/// way a cmd-shim would, splitting the interpreter's arguments like the
/// shell that would have run the shim.
#[cfg(unix)]
#[test]
fn global_fallback_preserves_quoted_shebang_arguments() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("global/node_modules/tool/cli");
    let interpreter = root.path().join("capture-argv");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&interpreter, "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\n").unwrap();
    fs::set_permissions(&interpreter, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&target, format!("#!{} --label \"value with spaces\"\n", interpreter.display()))
        .unwrap();
    let cwd = root.path().join("outside");
    fs::create_dir_all(&cwd).unwrap();

    let output = shim_command(&root, &cwd, "tool", target.to_str().unwrap())
        .arg("forwarded with spaces")
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let lines: Vec<_> =
        String::from_utf8_lossy(&output.stdout).lines().map(str::to_string).collect();
    assert_eq!(lines.first().map(String::as_str), Some("<--label>"));
    assert_eq!(lines.get(1).map(String::as_str), Some("<value with spaces>"));
    assert_eq!(lines.get(2).map(String::as_str), Some(format!("<{}>", target.display()).as_str()));
    assert_eq!(lines.last().map(String::as_str), Some("<forwarded with spaces>"));
}

/// A script target whose shebang names `node` runs on the bin dir's own
/// `node` when there is one, so a global tool follows the same
/// project-aware Node.js as a bare `node` would.
#[cfg(unix)]
#[test]
fn global_fallback_prefers_the_sibling_interpreter() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("global/node_modules/tool/cli.js");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "#!/usr/bin/env node\n").unwrap();
    write_script(&root.path().join("global-bin/node"), "sibling node");
    let cwd = root.path().join("outside");
    fs::create_dir_all(&cwd).unwrap();

    let output =
        shim_command(&root, &cwd, "tool", target.to_str().unwrap()).arg("--flag").output().unwrap();

    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "sibling node");
}

/// No shell sits between the caller and the target, so an environment
/// entry whose name is not a shell identifier reaches the target intact.
#[cfg(unix)]
#[test]
fn native_shim_preserves_non_shell_identifier_environment_variables() {
    let root = tempfile::tempdir().unwrap();
    let printenv = which::which("printenv").expect("the test needs `printenv` on PATH");

    let output = shim_command(&root, root.path(), "node", printenv.to_str().unwrap())
        .arg("TEST-VAR")
        .env("TEST-VAR", "123")
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "123");
}

/// A shim whose target is not recorded, or points back at the shim, must
/// fail rather than run the CLI or recurse.
#[cfg(unix)]
#[test]
fn a_shim_without_a_readable_target_fails() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("outside");
    fs::create_dir_all(&cwd).unwrap();
    let shim = shim_command(&root, &cwd, "tool", "placeholder").get_program().to_owned();
    fs::write(root.path().join("global-bin/.pnpm-shim-v1-tool-target"), b"pkg:not a name").unwrap();

    let output = Command::new(&shim).with_current_dir(&cwd).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read the global target"));

    let output = shim_command(&root, &cwd, "tool", shim.to_str().unwrap()).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("points back at the shim"));
}

#[cfg(windows)]
fn windows_shim_command(root: &TempDir, cwd: &Path, name: &str, target: &Path) -> Command {
    use std::os::windows::ffi::OsStrExt as _;
    let encoded = target.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    let shim = install_shim(&root.path().join("global-bin"), name, &encoded);
    Command::new(shim)
        .without_ambient_pnpm_config()
        .with_current_dir(cwd)
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
}

/// A `.cmd` target dispatches to the project's own copy and falls back
/// to the global one, in both cases through `cmd.exe`'s own argument
/// handling.
#[cfg(windows)]
#[test]
fn native_shim_dispatches_and_falls_back_to_cmd_targets() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let outside = root.path().join("outside");
    let local_package = project.join("node_modules/tool");
    let local_target = local_package.join("cli.cmd");
    let local_bin = project.join("node_modules/.bin");
    let global_package = root.path().join("global/node_modules/tool");
    let global_target = global_package.join("cli.cmd");
    fs::create_dir_all(&local_package).unwrap();
    fs::create_dir_all(&local_bin).unwrap();
    fs::create_dir_all(&global_package).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(local_package.join("package.json"), r#"{"name":"tool"}"#).unwrap();
    fs::write(global_package.join("package.json"), r#"{"name":"tool"}"#).unwrap();
    fs::write(&local_target, "@ECHO local:%*\r\n").unwrap();
    fs::write(&global_target, "@ECHO global:%*\r\n").unwrap();
    fs::write(local_bin.join("tool"), format!("# cmd-shim-target={}\n", local_target.display()))
        .unwrap();
    fs::write(local_bin.join("tool.cmd"), format!("@CALL \"{}\" %*\r\n", local_target.display()))
        .unwrap();

    let local = windows_shim_command(&root, &project, "tool", &global_target)
        .arg("value with spaces")
        .env(AUTO_TRUST_ENV, "1")
        .env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
        .output()
        .unwrap();
    let local_stdout = String::from_utf8_lossy(&local.stdout);
    assert!(
        local.status.success() && local_stdout.contains("local:"),
        "stdout:\n{local_stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&local.stderr),
    );
    assert!(local_stdout.contains("value with spaces"), "stdout:\n{local_stdout}");

    let global = windows_shim_command(&root, &outside, "tool", &global_target)
        .arg("value with spaces")
        .output()
        .unwrap();
    let global_stdout = String::from_utf8_lossy(&global.stdout);
    assert!(
        global.status.success() && global_stdout.contains("global:"),
        "stdout:\n{global_stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&global.stderr),
    );
    assert!(global_stdout.contains("value with spaces"), "stdout:\n{global_stdout}");
}

#[cfg(windows)]
#[test]
fn native_shim_runs_the_global_executable_fallback() {
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let global_target = std::env::var_os("ComSpec").expect("ComSpec should identify cmd.exe");

    let output = windows_shim_command(&root, &outside, "node", Path::new(&global_target))
        .args(["/d", "/c", "echo native-fallback"])
        .env("PNPM_CONFIG_GLOBAL_SHIMS", "true")
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("native-fallback"));
}

/// The bin dir a self-update by an earlier pnpm 12 leaves behind: the
/// shell shims it wrote, dispatching through a `.pnpm-shim-v1` that is now
/// the executable under test.
#[cfg(unix)]
fn install_legacy_shim(global_bin: &Path, name: &str, target: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(global_bin).unwrap();
    let dispatcher = global_bin.join(".pnpm-shim-v1");
    if !dispatcher.exists() {
        fs::copy(assert_cmd::cargo::cargo_bin("pnpm"), &dispatcher).unwrap();
    }
    let shim = global_bin.join(name);
    fs::write(
        &shim,
        format!(
            "#!/bin/sh\nbasedir=$(dirname \"$0\")\nif [ -z \"$PNPM_SHIM_BYPASS\" ] && [ -x \"$basedir/.pnpm-shim-v1\" ]; then\n  exec \"$basedir/.pnpm-shim-v1\" --shim '{name}' \"$basedir/\"'{name}' \"{target}\" -- \"$@\"\nfi\n\"{target}\" \"$@\"\nexit $?\n# pnpm-shim-style=context-aware\n# cmd-shim-target={target}\n",
        ),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    shim
}

#[cfg(unix)]
#[test]
fn legacy_shim_launch_dispatches_and_migrates_the_bin_dir() {
    let root = tempfile::tempdir().unwrap();
    let (project, global_target) = prepare_local_and_global(&root, "tool");
    let global_bin = root.path().join("global-bin");
    let other = install_legacy_shim(&global_bin, "other", "pkg:other");
    let shim = install_legacy_shim(&global_bin, "tool", global_target.to_str().unwrap());
    fs::write(project.join("node_modules/tool/cli.sh"), "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\n")
        .unwrap();
    let launch = |shim: &Path| {
        Command::new(shim)
            .without_ambient_pnpm_config()
            .without_env("PNPM_SHIM_BYPASS")
            .with_current_dir(&project)
            .with_env("PNPM_HOME", root.path().join("pnpm-home"))
            .with_env("XDG_STATE_HOME", root.path().join("state"))
            .with_env("XDG_CONFIG_HOME", root.path().join("config"))
            .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
            .with_env(AUTO_TRUST_ENV, "1")
            .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
            .with_args(["--flag", "value with spaces"])
            .output()
            .unwrap()
    };

    let first = launch(&shim);
    assert!(first.status.success(), "stderr:\n{}", String::from_utf8_lossy(&first.stderr));
    assert_eq!(String::from_utf8_lossy(&first.stdout).trim(), "<--flag>\n<value with spaces>");

    let executable_len = fs::metadata(assert_cmd::cargo::cargo_bin("pnpm")).unwrap().len();
    for name in ["tool", "other"] {
        assert_eq!(
            fs::metadata(global_bin.join(name)).unwrap().len(),
            executable_len,
            "the legacy {name} shim must have become the executable",
        );
    }
    assert_eq!(
        fs::read(global_bin.join(".pnpm-shim-v1-tool-target")).unwrap(),
        global_target.as_os_str().as_encoded_bytes(),
    );
    assert_eq!(fs::read(global_bin.join(".pnpm-shim-v1-other-target")).unwrap(), b"pkg:other");
    assert!(!global_bin.join(".pnpm-shim-v1").exists());

    let second = launch(&shim);
    assert!(second.status.success(), "stderr:\n{}", String::from_utf8_lossy(&second.stderr));
    assert_eq!(String::from_utf8_lossy(&second.stdout).trim(), "<--flag>\n<value with spaces>");
    let unprovided = launch(&other);
    assert!(!unprovided.status.success());
    assert!(String::from_utf8_lossy(&unprovided.stderr).contains("ERR_PNPM_SHIM_NO_TARGET"));
}

#[cfg(unix)]
#[test]
fn legacy_dispatcher_rejects_a_shim_from_another_directory() {
    let root = tempfile::tempdir().unwrap();
    let dispatcher_bin = root.path().join("dispatcher-bin");
    install_legacy_shim(&dispatcher_bin, "own", "pkg:own");
    let foreign_bin = root.path().join("foreign-bin");
    let foreign_shim = install_legacy_shim(&foreign_bin, "tool", "/bin/true");

    let output = Command::new(dispatcher_bin.join(".pnpm-shim-v1"))
        .arg("--shim")
        .arg("tool")
        .arg(&foreign_shim)
        .arg("/bin/true")
        .arg("--")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("legacy shim path") && stderr.contains("executing dispatcher"),
        "stderr:\n{stderr}",
    );
    assert!(foreign_bin.join(".pnpm-shim-v1").exists());
    assert!(fs::read(&foreign_shim).unwrap().starts_with(b"#!"));
}

#[cfg(unix)]
#[test]
fn legacy_shim_dispatches_without_waiting_for_the_migration_lock() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    write_script(&target, "launched");
    let global_bin = root.path().join("global-bin");
    let shim = install_legacy_shim(&global_bin, "tool", target.to_str().unwrap());
    let lock_dir = global_bin.join(".pnpm-global-bin.lock");
    fs::create_dir(&lock_dir).unwrap();
    fs::write(lock_dir.join("owner"), "held-by-test").unwrap();

    let output = Command::new(&shim)
        .without_ambient_pnpm_config()
        .without_env("PNPM_SHIM_BYPASS")
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "launched");
    assert!(global_bin.join(".pnpm-shim-v1").exists());
    assert!(fs::read(&shim).unwrap().starts_with(b"#!"));
}

/// A shim an earlier pnpm 12 wrote for the package is migrated before the
/// slot check, so re-adding the package repairs it instead of reporting a
/// conflict.
#[cfg(unix)]
#[test]
fn adding_a_shim_migrates_the_legacy_shim_for_the_same_package() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let global_bin = root.path().join("pnpm-home").join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let dispatcher = global_bin.join(".pnpm-shim-v1");
    fs::write(&dispatcher, "#!/bin/sh\nexit 1\n").unwrap();
    let legacy = global_bin.join("yarn");
    fs::write(
        &legacy,
        "#!/bin/sh\nexit 1\n# pnpm-shim-style=context-aware\n# cmd-shim-target=pkg:yarn\n",
    )
    .unwrap();
    fs::set_permissions(&legacy, fs::Permissions::from_mode(0o755)).unwrap();

    let added = pnpm_command(&root, &project).with_args(["shim", "add", "yarn"]).output().unwrap();

    assert!(stdout_of(&added).contains("yarn, yarnpkg"));
    assert_eq!(fs::read(global_bin.join(".pnpm-shim-v1-yarn-target")).unwrap(), b"pkg:yarn");
    assert_eq!(
        fs::metadata(&legacy).unwrap().len(),
        fs::metadata(assert_cmd::cargo::cargo_bin("pnpm")).unwrap().len(),
        "the legacy shell shim must have become the executable",
    );
    assert!(!dispatcher.exists());
}

/// Removing a package's shims also finds the one an earlier pnpm 12 wrote.
#[cfg(unix)]
#[test]
fn removing_a_shim_migrates_the_legacy_shim_first() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let global_bin = root.path().join("pnpm-home").join("bin");
    fs::create_dir_all(&global_bin).unwrap();
    let legacy = global_bin.join("yarn");
    fs::write(
        &legacy,
        "#!/bin/sh\nexit 1\n# pnpm-shim-style=context-aware\n# cmd-shim-target=pkg:yarn\n",
    )
    .unwrap();

    let removed = pnpm_command(&root, &project).with_args(["shim", "rm", "yarn"]).output().unwrap();

    assert!(stdout_of(&removed).contains("Removed yarn"));
    assert!(!legacy.exists());
    assert!(!global_bin.join(".pnpm-shim-v1-yarn-target").exists());
}

/// A `pnpm` invocation against an isolated pnpm home, for the commands
/// that manage shims rather than dispatch through one.
fn pnpm_command(root: &TempDir, cwd: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .unwrap()
        .without_ambient_pnpm_config()
        .with_current_dir(cwd)
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
}

fn stdout_of(output: &std::process::Output) -> String {
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `pnpm shim add` writes shims for a package that is not installed at
/// all, records the opt-in that governs them, and `rm` undoes both.
#[test]
fn shims_can_be_added_and_removed_for_a_package_that_is_not_installed() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let global_bin = root.path().join("pnpm-home").join("bin");

    let added = pnpm_command(&root, &project).with_args(["shim", "add", "yarn"]).output().unwrap();
    assert!(stdout_of(&added).contains("yarn, yarnpkg"));
    let exe = std::env::consts::EXE_SUFFIX;
    assert!(global_bin.join(format!("yarn{exe}")).exists());
    assert!(global_bin.join(format!("yarnpkg{exe}")).exists());
    let config =
        fs::read_to_string(root.path().join("config/pnpm/config.yaml")).expect("read config.yaml");
    assert!(config.contains("yarn: auto"), "{config}");

    let listed = pnpm_command(&root, &project).with_args(["shim", "ls"]).output().unwrap();
    assert!(stdout_of(&listed).contains("yarn (auto): yarn, yarnpkg"));

    let removed = pnpm_command(&root, &project).with_args(["shim", "rm", "yarn"]).output().unwrap();
    assert!(stdout_of(&removed).contains("Removed yarn, yarnpkg"));
    let left: Vec<_> = fs::read_dir(&global_bin)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("yarn"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    let config =
        fs::read_to_string(root.path().join("config/pnpm/config.yaml")).expect("read config.yaml");
    assert!(!config.contains("yarn: auto"), "{config}");
}

/// `globalShims: false` turns every context-aware shim off, so a shim
/// added under it would sit on `PATH` doing nothing — and recording the
/// opt-in would replace that off switch with a record that turns the
/// built-in runtime shims back on. The command says so instead.
#[test]
fn adding_a_shim_under_a_global_disable_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let config_dir = root.path().join("config").join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), "globalShims: false\n").unwrap();

    let refused =
        pnpm_command(&root, &project).with_args(["shim", "add", "yarn"]).output().unwrap();

    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("ERR_PNPM_SHIMS_DISABLED"), "{stderr}");
    assert_eq!(fs::read_to_string(config_dir.join("config.yaml")).unwrap(), "globalShims: false\n");
    // Nothing was linked at all — not the bare shim, not a Windows
    // flavor, not the dispatcher beside them.
    let global_bin = root.path().join("pnpm-home").join("bin");
    let linked: Vec<_> = fs::read_dir(&global_bin)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert!(linked.is_empty(), "{linked:?}");

    // Removing shims under it still works, and still leaves the setting
    // as the user wrote it.
    let removed = pnpm_command(&root, &project).with_args(["shim", "rm", "yarn"]).output().unwrap();
    assert!(removed.status.success(), "{}", String::from_utf8_lossy(&removed.stderr));
    assert_eq!(fs::read_to_string(config_dir.join("config.yaml")).unwrap(), "globalShims: false\n");
}

/// Removing a shim the record never held changes nothing, so it writes
/// nothing — spelling the built-in defaults into the user's
/// configuration is not what `pnpm shim rm` was asked to do.
#[test]
fn removing_a_shim_that_was_never_recorded_leaves_the_config_alone() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let config = root.path().join("config").join("pnpm").join("config.yaml");

    let removed = pnpm_command(&root, &project).with_args(["shim", "rm", "yarn"]).output().unwrap();

    assert!(removed.status.success(), "{}", String::from_utf8_lossy(&removed.stderr));
    assert!(stdout_of(&removed).contains("No shims for yarn"));
    assert!(!config.exists(), "{}", fs::read_to_string(&config).unwrap_or_default());
}

/// Switching a built-in shim off is done by recording it off, so
/// clearing that entry would switch it back on. `pnpm shim rm` never
/// enables anything.
#[test]
fn removing_a_disabled_shim_does_not_enable_it() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let config_dir = root.path().join("config").join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), "globalShims:\n  node: false\n").unwrap();

    let removed = pnpm_command(&root, &project).with_args(["shim", "rm", "node"]).output().unwrap();

    assert!(removed.status.success(), "{}", String::from_utf8_lossy(&removed.stderr));
    assert_eq!(
        fs::read_to_string(config_dir.join("config.yaml")).unwrap(),
        "globalShims:\n  node: false\n",
    );
}

/// A package that dispatches nothing by default is a different matter:
/// its off entry enables nothing when cleared, so it is the user's to
/// take back.
#[test]
fn removing_a_disabled_shim_clears_it_when_nothing_would_switch_on() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let config_dir = root.path().join("config").join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), "globalShims:\n  typescript: false\n").unwrap();

    let removed =
        pnpm_command(&root, &project).with_args(["shim", "rm", "typescript"]).output().unwrap();

    assert!(removed.status.success(), "{}", String::from_utf8_lossy(&removed.stderr));
    let config = fs::read_to_string(config_dir.join("config.yaml")).unwrap();
    assert!(!config.contains("typescript"), "{config}");
}

/// Removing a shim while a higher-precedence disable is active rewrites
/// only what the user's own record held: the built-in defaults are not
/// spelled into it, so lifting that disable later enables nothing the
/// record did not already enable.
#[test]
fn removing_a_shim_under_a_higher_precedence_disable_records_no_defaults() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(&pnpm_home).unwrap();
    let config_dir = root.path().join("config").join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();

    // The record the user has, and a disable that outranks it.
    fs::write(config_dir.join("config.yaml"), "globalShims:\n  yarn: auto\n  node: false\n")
        .unwrap();
    fs::write(pnpm_home.join("pnpm-workspace.yaml"), "globalShims: false\n").unwrap();

    let removed = pnpm_command(&root, &project).with_args(["shim", "rm", "yarn"]).output().unwrap();

    assert!(removed.status.success(), "{}", String::from_utf8_lossy(&removed.stderr));
    let config = fs::read_to_string(config_dir.join("config.yaml")).unwrap();
    assert!(!config.contains("yarn"), "{config}");
    // The rest of the record stands, and nothing else was added to it.
    assert!(config.contains("node: false"), "{config}");
    for defaulted in ["deno", "bun"] {
        assert!(!config.contains(defaulted), "{defaulted} was written into {config}");
    }
}

/// The pnpm home's `pnpm-workspace.yaml` and the environment both outrank
/// the file `pnpm shim` writes into, so a disable in either is a disable:
/// the shim would sit on `PATH` doing nothing.
#[test]
fn adding_a_shim_under_a_higher_precedence_disable_is_refused() {
    for (label, disable) in [("workspace yaml", true), ("environment", false)] {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let pnpm_home = root.path().join("pnpm-home");
        fs::create_dir_all(&pnpm_home).unwrap();

        let mut command = pnpm_command(&root, &project);
        if disable {
            fs::write(pnpm_home.join("pnpm-workspace.yaml"), "globalShims: false\n").unwrap();
        } else {
            command = command.with_env("PNPM_CONFIG_GLOBAL_SHIMS", "false");
        }
        let refused = command.with_args(["shim", "add", "yarn"]).output().unwrap();

        assert!(!refused.status.success(), "{label}");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(stderr.contains("ERR_PNPM_SHIMS_DISABLED"), "{label}: {stderr}");
        // Nothing was linked: not the shim, not a Windows flavor of it,
        // not the dispatcher beside them.
        let linked: Vec<_> = fs::read_dir(pnpm_home.join("bin"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert!(linked.is_empty(), "{label}: {linked:?}");
    }
}

/// Re-adding a package's own shims is how they get repaired, but a bin
/// something else already provides is not this command's to take: it
/// would break that command, and `pnpm shim rm` would then delete it
/// rather than give it back.
#[test]
fn adding_a_shim_over_another_package_s_bin_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let global_bin = root.path().join("pnpm-home").join("bin");

    let added = pnpm_command(&root, &project).with_args(["shim", "add", "yarn"]).output().unwrap();
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stderr));
    let readded =
        pnpm_command(&root, &project).with_args(["shim", "add", "yarn"]).output().unwrap();
    assert!(readded.status.success(), "{}", String::from_utf8_lossy(&readded.stderr));

    let removed = pnpm_command(&root, &project).with_args(["shim", "rm", "yarn"]).output().unwrap();
    assert!(removed.status.success(), "{}", String::from_utf8_lossy(&removed.stderr));
    assert!(!global_bin.join("yarn").exists());
    let installed_globally = "#!/bin/sh\necho a global install\n";
    fs::write(global_bin.join("yarn"), installed_globally).unwrap();
    let refused =
        pnpm_command(&root, &project).with_args(["shim", "add", "yarn"]).output().unwrap();

    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("ERR_PNPM_SHIM_BIN_CONFLICT"), "{stderr}");
    assert_eq!(fs::read_to_string(global_bin.join("yarn")).unwrap(), installed_globally);
}

/// Nothing installed the package behind the shim, so a project that
/// neither pins nor depends on it has nothing to run — and the shim says
/// so instead of falling through to whatever else is on `PATH`.
#[cfg(unix)]
#[test]
fn a_shim_without_a_project_target_reports_it() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("package.json"), r#"{"name":"project","version":"1.0.0"}"#).unwrap();

    let added = pnpm_command(&root, &project).with_args(["shim", "add", "yarn"]).output().unwrap();
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stderr));
    let output = Command::new(root.path().join("pnpm-home").join("bin").join("yarn"))
        .without_ambient_pnpm_config()
        .with_current_dir(&project)
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        // The dispatched shim must not reach into the developer's cache
        // either, whatever it decides to run.
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
        .with_arg("--version")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SHIM_NO_TARGET"), "{stderr}");
    assert!(stderr.contains("yarn"), "{stderr}");
}
