#[cfg(windows)]
use super::windows_shim_command;
use super::{AUTO_TRUST_ENV, Path, fs};
#[cfg(unix)]
use super::{
    Command, CommandExtra, CommandTestExt, install_legacy_shim, pnpm_command,
    prepare_local_and_global, shim_command, stdout_of, write_script,
};

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
    pnpm_testing_utils::diagnostics::assert_diagnostic_contains(&stderr, "legacy shim path");
    pnpm_testing_utils::diagnostics::assert_diagnostic_contains(&stderr, "executing dispatcher");
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
