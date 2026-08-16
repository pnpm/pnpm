//! End-to-end tests for context-aware global shim dispatch
//! (`pnpm --shim <name> <shim> <target> -- <args>`).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const AUTO_TRUST_ENV: &str = "PNPM_AUTO_APPROVE_PROJECT_BINS_FOR_TESTS";

/// A `pnpm --shim` invocation with an isolated pnpm home and state dir so
/// the dispatcher can neither see the developer's global installs nor
/// their trust registry.
fn shim_command(root: &TempDir, cwd: &Path, shim_args: &[&str]) -> Command {
    let mut command = Command::cargo_bin("pnpm")
        .unwrap()
        .without_ambient_pnpm_config()
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
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
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
    let output = shim_command(
        &root,
        &project,
        &["tool", global_target.to_str().unwrap(), "--", "--flag", "value with spaces"],
    )
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

    shim_command(&root, &project, &["node", global_target.to_str().unwrap(), "--"])
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
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
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
    fs::write(pnpm_home.join("pnpm-workspace.yaml"), "globalShims: false\n").unwrap();
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

        let mut command =
            shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
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
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
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
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env(AUTO_TRUST_ENV, "1")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "global");
}

/// A fallback shim path the host cannot execute degrades to the embedded
/// target instead of failing the invocation.
#[cfg(unix)]
#[test]
fn unexecutable_fallback_shim_degrades_to_the_target() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("plain");
    fs::create_dir_all(&cwd).unwrap();
    let target = root.path().join("global").join("node_modules").join("tool").join("cli.sh");
    write_script(&target, "global");
    let broken_shim = root.path().join("broken-shim");
    fs::write(&broken_shim, "#!/bin/sh\necho never\n").unwrap();
    // Not executable: re-entry must fail and the target must run. Built
    // without `shim_command` because that helper injects its own
    // (nonexistent) shim path after the name.
    let output = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&cwd)
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", "false")
        .with_args([
            "--shim",
            "tool",
            broken_shim.to_str().unwrap(),
            target.to_str().unwrap(),
            "--",
        ])
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
    let output = shim_command(&root, &project, &["tool", global_target.to_str().unwrap(), "--"])
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": "always"}"#)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert_eq!(stdout.trim(), "local");
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
    use pnpm_cmd_shim::{Host, ShimStyle, generate_sh_shim, search_script_runtime};
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
    use pnpm_cmd_shim::{ShimStyle, generate_cmd_shim, generate_pwsh_shim};

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

    let direct = Command::cargo_bin("pnpm")
        .unwrap()
        .args(["--shim", "tool"])
        .arg(&cmd_shim)
        .arg(&global_target)
        .args(["--", "value with spaces"])
        .current_dir(&project)
        .env(AUTO_TRUST_ENV, "1")
        .env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
        .env("PNPM_HOME", root.path().join("pnpm-home"))
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .output()
        .unwrap();
    let direct_stdout = String::from_utf8_lossy(&direct.stdout);
    assert!(
        direct.status.success() && direct_stdout.contains("local:"),
        "direct stdout:\n{direct_stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&direct.stderr),
    );

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
            .env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"tool": true}"#)
            .env("PNPM_HOME", root.path().join("pnpm-home"))
            .env("XDG_STATE_HOME", root.path().join("state"))
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .output()
            .unwrap();
        assert!(local.status.success(), "stderr:\n{}", String::from_utf8_lossy(&local.stderr));
        let local_stdout = String::from_utf8_lossy(&local.stdout);
        assert!(
            local_stdout.contains("local:"),
            "{shell} stdout:\n{local_stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&local.stderr),
        );
        assert!(local_stdout.contains("value with spaces"), "{shell} stdout:\n{local_stdout}");

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

    let fake_bin = root.path().join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    fs::copy(
        std::env::var_os("ComSpec").expect("ComSpec should identify cmd.exe"),
        fake_bin.join("powershell.exe"),
    )
    .unwrap();
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(fake_bin).chain(std::env::split_paths(&inherited_path)),
    )
    .unwrap();
    let system_powershell = Path::new(
        &std::env::var_os("SystemRoot").expect("SystemRoot should identify the Windows directory"),
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let global = Command::new(system_powershell)
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&pwsh_shim)
        .arg("path-hijack-check")
        .current_dir(&outside)
        .env("PATH", path)
        .env("PNPM_HOME", root.path().join("pnpm-home"))
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .output()
        .unwrap();
    let global_stdout = String::from_utf8_lossy(&global.stdout);
    assert!(
        global.status.success() && global_stdout.contains("global:path-hijack-check"),
        "stdout:\n{global_stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&global.stderr),
    );
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
        .env("PNPM_CONFIG_GLOBAL_SHIMS", "true")
        .env("PNPM_HOME", root.path().join("pnpm-home"))
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("native-fallback"));
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
    assert!(global_bin.join("yarn").exists());
    assert!(global_bin.join("yarnpkg").exists());
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
