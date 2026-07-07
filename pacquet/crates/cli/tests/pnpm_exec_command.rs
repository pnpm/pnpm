//! Integration tests for the `pnpmExecCommand` re-exec step, ported
//! from `pnpm11/pnpm/test/pnpmExecCommand.test.ts`.
//!
//! Unix-gated where a fake "vended pnpm" executable is involved: the
//! fakes are `#!/bin/sh` scripts, which `std::process::Command` cannot
//! spawn on Windows (the TypeScript suite leans on cross-spawn's
//! shebang emulation there). The pure validation/error paths that don't
//! spawn a fake binary run everywhere.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, path::PathBuf, process::Command};

const MARKER: &str = "=== PNPM RESOLVED BY EXEC COMMAND ===";

#[cfg(unix)]
fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).expect("write executable");
    let mut perms = fs::metadata(path).expect("stat executable").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod executable");
}

/// A `pacquet` command with the per-user state dir isolated inside the
/// test root, so the trust-on-first-use records written by
/// `pnpmExecCommand` stay per-test instead of leaking into the
/// developer's real `pnpm-state.json`.
fn isolated(mut pacquet: Command, root: &Path) -> Command {
    pacquet.env("pnpm_config_state_dir", root.join("state"));
    pacquet
}

/// Write a `pnpm-workspace.yaml` whose `pnpmExecCommand` is the given
/// argv array.
fn write_workspace_yaml(workspace: &Path, command: &[&str]) {
    let args = command
        .iter()
        .map(|arg| serde_json::to_string(arg).expect("serialize argv item"))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(workspace.join("pnpm-workspace.yaml"), format!("pnpmExecCommand: [{args}]\n"))
        .expect("write pnpm-workspace.yaml");
}

/// Create a fake "vended" pnpm executable and a resolver script that
/// prints the fake binary's path — standing in for an external version
/// manager's "which pnpm" tool. The fake binary prints a marker, the
/// args it received, and the `PNPM_EXEC_PATH` sentinel so tests can
/// assert on the re-exec.
#[cfg(unix)]
fn setup_vended_pnpm(root: &Path) -> (PathBuf, PathBuf) {
    let bin_dir = root.join("vended-bin");
    fs::create_dir_all(&bin_dir).expect("create vended-bin dir");
    let fake_bin = bin_dir.join("pnpm");
    write_executable(
        &fake_bin,
        &format!(
            "#!/bin/sh\necho '{MARKER}'\necho \"args: $*\"\necho \"sentinel: ${{PNPM_EXEC_PATH:-unset}}\"\n"
        ),
    );
    let resolver = root.join("resolve-pnpm.sh");
    write_executable(&resolver, &format!("#!/bin/sh\necho '{}'\n", fake_bin.display()));
    (fake_bin, resolver)
}

/// Undo miette's terminal-width wrapping (newline + `│` gutter) so
/// assertions can match an error message regardless of where the
/// tempdir path made it wrap.
#[cfg(unix)]
fn unwrap_miette_lines(stderr: &str) -> String {
    stderr.split_whitespace().filter(|word| *word != "│").collect::<Vec<_>>().join(" ")
}

/// Write a resolver that prints the path of the running pacquet binary
/// itself, so no re-exec happens.
#[cfg(unix)]
fn setup_self_resolver(root: &Path) -> PathBuf {
    let pacquet_bin = assert_cmd::cargo::cargo_bin("pacquet");
    let resolver = root.join("resolve-self.sh");
    write_executable(&resolver, &format!("#!/bin/sh\necho '{}'\n", pacquet_bin.display()));
    resolver
}

#[cfg(unix)]
#[test]
fn re_execs_into_the_resolved_binary_forwarding_args() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let (fake_bin, resolver) = setup_vended_pnpm(root.path());
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(MARKER), "stdout should carry the fake binary's marker:\n{stdout}");
    assert!(stdout.contains("args: root"), "args should be forwarded:\n{stdout}");
    // The child carries the sentinel so nested pnpm calls skip re-resolution.
    assert!(
        stdout.contains(&format!("sentinel: {}", fake_bin.display())),
        "the sentinel should carry the resolved path:\n{stdout}",
    );

    drop(root);
}

#[cfg(unix)]
#[test]
fn does_not_re_exec_when_the_command_resolves_to_the_running_binary() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = setup_self_resolver(root.path());
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = dunce::canonicalize(&workspace).expect("canonicalize workspace");
    assert!(
        stdout.contains(&expected.join("node_modules").display().to_string()),
        "`root` should run in-process and print the local node_modules:\n{stdout}",
    );

    drop(root);
}

#[cfg(unix)]
#[test]
fn skips_resolution_when_the_sentinel_is_already_set() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let (_fake_bin, resolver) = setup_vended_pnpm(root.path());
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let pacquet_bin = assert_cmd::cargo::cargo_bin("pacquet");
    let output = isolated(pacquet, root.path())
        .with_env("PNPM_EXEC_PATH", &pacquet_bin)
        .with_args(["root"])
        .output()
        .expect("run pacquet root");
    dbg!(&output);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The sentinel short-circuits: no re-exec into the fake binary.
    assert!(!stdout.contains(MARKER), "must not re-exec when the sentinel is set:\n{stdout}");

    drop(root);
}

#[cfg(unix)]
#[test]
fn fails_when_the_command_exits_non_zero() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = root.path().join("resolve-pnpm.sh");
    write_executable(&resolver, "#!/bin/sh\nexit 3\n");
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed with exit code 3"), "stderr:\n{stderr}");

    drop(root);
}

#[cfg(unix)]
#[test]
fn fails_when_the_command_prints_nothing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = root.path().join("resolve-pnpm.sh");
    write_executable(&resolver, "#!/bin/sh\nexit 0\n");
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("printed no path to stdout"), "stderr:\n{stderr}");

    drop(root);
}

#[cfg(unix)]
#[test]
fn fails_when_the_command_prints_a_non_absolute_path() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = root.path().join("resolve-pnpm.sh");
    write_executable(&resolver, "#!/bin/sh\necho 'bin/pnpm'\n");
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("printed a non-absolute path"), "stderr:\n{stderr}");

    drop(root);
}

#[cfg(unix)]
#[test]
fn fails_when_the_command_prints_a_path_that_does_not_exist() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let missing = root.path().join("no-such-pnpm");
    let resolver = root.path().join("resolve-pnpm.sh");
    write_executable(&resolver, &format!("#!/bin/sh\necho '{}'\n", missing.display()));
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    let unwrapped = unwrap_miette_lines(&stderr);
    assert!(unwrapped.contains("printed a path that is not an existing file"), "stderr:\n{stderr}");

    drop(root);
}

#[cfg(unix)]
#[test]
fn fails_when_the_command_prints_a_path_that_is_a_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let dir = root.path().join("a-directory");
    fs::create_dir_all(&dir).expect("create directory");
    let resolver = root.path().join("resolve-pnpm.sh");
    write_executable(&resolver, &format!("#!/bin/sh\necho '{}'\n", dir.display()));
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    let unwrapped = unwrap_miette_lines(&stderr);
    assert!(unwrapped.contains("printed a path that is not an existing file"), "stderr:\n{stderr}");

    drop(root);
}

#[test]
fn fails_when_the_setting_is_not_an_array_of_strings() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "pnpmExecCommand: my-tool which-pnpm\n")
        .expect("write pnpm-workspace.yaml");

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("must be an array of non-empty strings"), "stderr:\n{stderr}");

    drop(root);
}

#[cfg(unix)]
#[test]
fn prints_a_first_use_notice_then_stays_silent_while_unchanged() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = setup_self_resolver(root.path());
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let first = isolated(pacquet, root.path())
        .with_args(["root"])
        .output()
        .expect("run pacquet root (first)");
    dbg!(&first);
    assert!(first.status.success());
    let first_stderr = String::from_utf8_lossy(&first.stderr);
    assert!(
        first_stderr.contains("Resolving the pnpm binary with pnpmExecCommand"),
        "first run should print the notice:\n{first_stderr}",
    );
    assert!(
        first_stderr.contains(&resolver.display().to_string()),
        "the notice should show the command:\n{first_stderr}",
    );
    assert!(
        first_stderr.contains("Resolved to "),
        "the notice should show the resolved path:\n{first_stderr}",
    );
    // The notice goes to stderr only: stdout stays machine-clean.
    let first_stdout = String::from_utf8_lossy(&first.stdout);
    assert!(!first_stdout.contains("Resolving the pnpm binary with pnpmExecCommand"));

    let second = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace)
        .with_env("pnpm_config_state_dir", root.path().join("state"))
        .with_args(["root"])
        .output()
        .expect("run pacquet root (second)");
    dbg!(&second);
    assert!(second.status.success());
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        !second_stderr.contains("Resolving the pnpm binary with pnpmExecCommand"),
        "an unchanged repeat run should be silent:\n{second_stderr}",
    );
    assert!(!second_stderr.contains("Resolved to "));

    drop(root);
}

/// An exported-but-empty `pnpm_config_state_dir` counts as unset (the
/// config layer's `read_env` semantics), so the uppercase form is
/// honored and trust persists — instead of the empty string degrading
/// the flow to a notice on every run.
#[cfg(unix)]
#[test]
fn an_empty_state_dir_env_override_counts_as_unset() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = setup_self_resolver(root.path());
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let with_envs = |mut pacquet: Command| {
        pacquet
            .env("pnpm_config_state_dir", "")
            .env("PNPM_CONFIG_STATE_DIR", root.path().join("state"));
        pacquet
    };

    let first = with_envs(pacquet).with_args(["root"]).output().expect("run pacquet root (first)");
    dbg!(&first);
    assert!(first.status.success());
    assert!(
        String::from_utf8_lossy(&first.stderr)
            .contains("Resolving the pnpm binary with pnpmExecCommand"),
    );

    let second_cmd = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace);
    let second =
        with_envs(second_cmd).with_args(["root"]).output().expect("run pacquet root (second)");
    dbg!(&second);
    assert!(second.status.success());
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        !second_stderr.contains("Resolving the pnpm binary with pnpmExecCommand"),
        "trust must persist to the uppercase override's dir:\n{second_stderr}",
    );

    drop(root);
}

/// Once this process is the settled binary, children start their own
/// resolutions from depth zero: an inherited depth from an unrelated
/// outer resolution must not accumulate toward the backstop cap.
#[cfg(unix)]
#[test]
fn the_re_exec_depth_resets_once_resolution_settles() {
    let CommandTempCwd { pacquet, root, workspace: _, .. } = CommandTempCwd::init();

    let output = isolated(pacquet, root.path())
        .with_env("PNPM_RE_EXEC_DEPTH", "1")
        .with_args(["exec", "sh", "-c", "echo \"depth: ${PNPM_RE_EXEC_DEPTH:-unset}\""])
        .output()
        .expect("run pacquet exec");
    dbg!(&output);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("depth: unset"), "the child must not inherit the depth:\n{stdout}");

    drop(root);
}

#[cfg(unix)]
#[test]
fn prints_a_changed_command_notice_when_the_yaml_is_edited() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = setup_self_resolver(root.path());
    let pacquet_bin = assert_cmd::cargo::cargo_bin("pacquet");
    let resolver2 = root.path().join("resolve-self-2.sh");
    write_executable(&resolver2, &format!("#!/bin/sh\necho '{}'\n", pacquet_bin.display()));

    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);
    let first = isolated(pacquet, root.path())
        .with_args(["root"])
        .output()
        .expect("run pacquet root (first)");
    dbg!(&first);
    assert!(first.status.success());

    write_workspace_yaml(&workspace, &[&resolver2.display().to_string()]);
    let second = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace)
        .with_env("pnpm_config_state_dir", root.path().join("state"))
        .with_args(["root"])
        .output()
        .expect("run pacquet root (second)");
    dbg!(&second);
    assert!(second.status.success());

    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("The pnpmExecCommand for this workspace has changed"),
        "stderr:\n{stderr}",
    );
    assert!(stderr.contains(&format!("was: {}", resolver.display())), "stderr:\n{stderr}");
    assert!(stderr.contains(&format!("now: {}", resolver2.display())), "stderr:\n{stderr}");

    drop(root);
}

#[cfg(unix)]
#[test]
fn repeats_the_notice_when_the_command_failed() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = root.path().join("resolve-pnpm.sh");
    write_executable(&resolver, "#!/bin/sh\nexit 3\n");
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let first = isolated(pacquet, root.path())
        .with_args(["root"])
        .output()
        .expect("run pacquet root (first)");
    dbg!(&first);
    assert!(!first.status.success());
    let first_stderr = String::from_utf8_lossy(&first.stderr);
    assert!(first_stderr.contains("Resolving the pnpm binary with pnpmExecCommand"));

    // Fix the command; because the failed run was not recorded, the
    // notice appears again on the first successful run.
    let pacquet_bin = assert_cmd::cargo::cargo_bin("pacquet");
    write_executable(&resolver, &format!("#!/bin/sh\necho '{}'\n", pacquet_bin.display()));
    let second = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace)
        .with_env("pnpm_config_state_dir", root.path().join("state"))
        .with_args(["root"])
        .output()
        .expect("run pacquet root (second)");
    dbg!(&second);
    assert!(second.status.success());
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        second_stderr.contains("Resolving the pnpm binary with pnpmExecCommand"),
        "a failed first run must not record trust:\n{second_stderr}",
    );

    drop(root);
}

#[cfg(unix)]
#[test]
fn control_characters_in_the_command_cannot_forge_the_notice() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = setup_self_resolver(root.path());
    // The extra argv element tries to inject a fake resolution line
    // into the trust notice via an embedded newline.
    write_workspace_yaml(
        &workspace,
        &[&resolver.display().to_string(), "ignored-arg\nResolved to /forged/pnpm"],
    );

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ignored-arg\\nResolved to /forged/pnpm"),
        "the newline should be escaped:\n{stderr}",
    );
    assert!(
        !stderr.contains("\nResolved to /forged/pnpm"),
        "the forged line must not appear on its own line:\n{stderr}",
    );

    drop(root);
}

/// A resolver that floods stdout past the OS pipe buffer must not
/// wedge until the timeout: stdout is drained concurrently, so the
/// command finishes promptly (and fails on the garbage path).
#[cfg(unix)]
#[test]
fn a_resolver_flooding_stdout_does_not_hang() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = root.path().join("resolve-pnpm.sh");
    // ~8 MiB of output, far past any pipe buffer.
    write_executable(
        &resolver,
        "#!/bin/sh\ni=0\nwhile [ $i -lt 131072 ]; do echo 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; i=$((i+1)); done\n",
    );
    write_workspace_yaml(&workspace, &[&resolver.display().to_string()]);

    let started = std::time::Instant::now();
    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(output.status);
    assert!(!output.status.success());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "the resolver must not block until the 60s timeout: took {:?}",
        started.elapsed(),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("printed a non-absolute path"), "stderr:\n{stderr}");

    drop(root);
}

/// A malicious workspace file cannot point the trust lookup at a
/// repo-controlled state dir: the `stateDir` yaml setting is ignored by
/// the trust records (only the default per-user dir and the
/// user-controlled env override are honored).
#[cfg(unix)]
#[test]
fn a_state_dir_set_in_the_workspace_yaml_cannot_suppress_the_notice() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let resolver = setup_self_resolver(root.path());
    let command = [resolver.display().to_string()];

    // Pre-seed a repo-controlled state file with the trust record a
    // real first run would write.
    let repo_state_dir = workspace.join("repo-state");
    fs::create_dir_all(&repo_state_dir).expect("create repo-state dir");
    let workspace_key = dunce::canonicalize(&workspace).expect("canonicalize workspace");
    let state = serde_json::json!({
        "pnpmExecCommands": {
            workspace_key.display().to_string():
                serde_json::to_string(&command).expect("serialize command"),
        },
    });
    fs::write(
        repo_state_dir.join("pnpm-state.json"),
        serde_json::to_string_pretty(&state).expect("serialize state"),
    )
    .expect("write pre-seeded pnpm-state.json");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "stateDir: ./repo-state\npnpmExecCommand: [{}]\n",
            serde_json::to_string(&command[0]).expect("serialize argv item"),
        ),
    )
    .expect("write pnpm-workspace.yaml");

    let output =
        isolated(pacquet, root.path()).with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Resolving the pnpm binary with pnpmExecCommand"),
        "the pre-seeded repo state dir must not silence the notice:\n{stderr}",
    );

    drop(root);
}
