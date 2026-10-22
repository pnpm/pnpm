//! `--env-file <path>` — dotenv files layered into the process environment.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{fs, process::Command};

fn pacquet_in(workspace: &std::path::Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn write_manifest(workspace: &std::path::Path) {
    let manifest = serde_json::json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "print-env": r#"node -e "process.stdout.write([process.env.PACQUET_ENV_FILE_SCRIPT, process.env.PACQUET_ENV_FILE_EXPORTED, process.env.PACQUET_ENV_FILE_QUOTED].join('|'))""#,
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

/// Variables from `--env-file` reach lifecycle scripts, including `export`-
/// prefixed and quoted entries. Invoked through the `pnpm <script>`
/// fallback, so the flag is claimed before the script boundary.
#[test]
fn script_sees_env_file_variables() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace);
    fs::write(
        workspace.join(".env"),
        "PACQUET_ENV_FILE_SCRIPT=from-env-file\nexport PACQUET_ENV_FILE_EXPORTED=exported-value\nPACQUET_ENV_FILE_QUOTED=\"quoted value\"\n",
    )
    .expect("write .env");

    let output = pacquet_in(&workspace)
        .with_args(["--env-file", ".env", "print-env"])
        .output()
        .expect("run pacquet with --env-file");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "command must exit 0, got: {output:?}");
    assert!(
        stdout.contains("from-env-file|exported-value|quoted value"),
        "script must see the layered variables; stdout: {stdout:?}",
    );

    drop(root);
}

/// Repeatable: later files override variables defined in earlier files,
/// matching `node --env-file`.
#[test]
fn later_env_file_overrides_earlier_env_file() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace);
    fs::write(
        workspace.join("first.env"),
        "PACQUET_ENV_FILE_SCRIPT=first\nPACQUET_ENV_FILE_EXPORTED=kept-from-first\n",
    )
    .expect("write first.env");
    fs::write(workspace.join("second.env"), "PACQUET_ENV_FILE_SCRIPT=second\n")
        .expect("write second.env");

    let output = pacquet_in(&workspace)
        .with_args(["--env-file", "first.env", "--env-file=second.env", "run", "print-env"])
        .output()
        .expect("run pacquet with layered --env-file");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "command must exit 0, got: {output:?}");
    assert!(
        stdout.contains("second|kept-from-first|"),
        "later file must override earlier file; stdout: {stdout:?}",
    );

    drop(root);
}

/// A variable already in the environment is never overridden by the file.
#[test]
fn existing_environment_wins_over_env_file() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace);
    fs::write(workspace.join(".env"), "PACQUET_ENV_FILE_SCRIPT=from-env-file\n")
        .expect("write .env");

    let mut command = pacquet_in(&workspace);
    command.env("PACQUET_ENV_FILE_SCRIPT", "from-process");
    let output = command
        .with_args(["--env-file", ".env", "run", "print-env"])
        .output()
        .expect("run pacquet with --env-file");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "command must exit 0, got: {output:?}");
    assert!(stdout.contains("from-process|"), "process environment must win; stdout: {stdout:?}");

    drop(root);
}

/// A `pm` prefix still loads `--env-file`: the flag after `pm` is pnpm's own
/// option, not a forwarded script argument.
#[test]
fn pm_prefix_still_loads_env_file() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace);
    fs::write(workspace.join(".env"), "PACQUET_ENV_FILE_SCRIPT=from-env-file\n")
        .expect("write .env");

    let output = pacquet_in(&workspace)
        .with_args(["pm", "--env-file", ".env", "run", "print-env"])
        .output()
        .expect("run pacquet with pm prefix and --env-file");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "command must exit 0, got: {output:?}");
    assert!(
        stdout.contains("from-env-file|"),
        "script must see the layered variables; stdout: {stdout:?}",
    );

    drop(root);
}

/// A missing file fails the command and names the file.
#[test]
fn missing_env_file_fails_loudly() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet_in(&workspace)
        .with_args(["--env-file", "does-not-exist.env", "get", "registry"])
        .output()
        .expect("run pacquet with a missing --env-file");
    assert!(!output.status.success(), "missing file must fail: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does-not-exist.env"), "error must name the file: {stderr}");

    drop(root);
}

/// A malformed file fails the command and names the file, without echoing
/// its contents (env files routinely hold secrets).
#[test]
fn malformed_env_file_fails_loudly() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("malformed.env"),
        "PACQUET_ENV_FILE_TEST_REDACTED_SECRET no equals here\n",
    )
    .expect("write malformed.env");

    let output = pacquet_in(&workspace)
        .with_args(["--env-file", "malformed.env", "get", "registry"])
        .output()
        .expect("run pacquet with a malformed --env-file");
    assert!(!output.status.success(), "malformed file must fail: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("malformed.env"), "error must name the file: {stderr}");
    assert!(
        !stderr.contains("PACQUET_ENV_FILE_TEST_REDACTED_SECRET"),
        "error must not echo file contents: {stderr}",
    );

    drop(root);
}

/// A value containing a NUL byte fails the command instead of panicking in
/// `set_var`, and names the file and the cause.
#[test]
fn nul_valued_env_file_fails_loudly() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("nul.env"), b"PACQUET_ENV_FILE_TEST_NUL=before\0after\n")
        .expect("write nul.env");

    let output = pacquet_in(&workspace)
        .with_args(["--env-file", "nul.env", "get", "registry"])
        .output()
        .expect("run pacquet with a NUL-valued --env-file");
    assert!(!output.status.success(), "NUL value must fail: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nul.env"), "error must name the file: {stderr}");
    assert!(stderr.contains("NUL"), "error must name the cause: {stderr}");
    assert!(!stderr.contains("PACQUET_ENV_FILE_TEST_NUL"), "error must not echo the key: {stderr}");

    drop(root);
}

/// A file with invalid UTF-8 fails as a malformed file, not a read error:
/// the file is readable, its contents are not valid dotenv input.
#[test]
fn invalid_utf8_env_file_fails_as_malformed() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("invalid-utf8.env"), b"PACQUET_ENV_FILE_TEST_UTF8=\xFF\n")
        .expect("write invalid-utf8.env");

    let output = pacquet_in(&workspace)
        .with_args(["--env-file", "invalid-utf8.env", "get", "registry"])
        .output()
        .expect("run pacquet with an invalid-UTF-8 --env-file");
    assert!(!output.status.success(), "invalid UTF-8 must fail: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid-utf8.env"), "error must name the file: {stderr}");
    assert!(stderr.contains("UTF-8"), "error must name the cause: {stderr}");
    assert!(
        !stderr.contains("Failed to read"),
        "a readable file must not report a read error: {stderr}",
    );

    drop(root);
}

/// A UTF-8 byte-order mark does not break the first entry: Windows editors
/// prepend one routinely, and dotenvy's own file loader tolerates it.
#[test]
fn bom_prefixed_env_file_loads() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace);
    let mut contents = vec![0xEF, 0xBB, 0xBF];
    contents.extend_from_slice(b"PACQUET_ENV_FILE_SCRIPT=from-bom-env\n");
    fs::write(workspace.join(".env"), contents).expect("write .env");

    let output = pacquet_in(&workspace)
        .with_args(["--env-file", ".env", "run", "print-env"])
        .output()
        .expect("run pacquet with a BOM-prefixed --env-file");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "command must exit 0, got: {output:?}");
    assert!(
        stdout.contains("from-bom-env|"),
        "script must see the BOM-prefixed variable; stdout: {stdout:?}",
    );

    drop(root);
}

/// Variables from `--env-file` resolve `${VAR}` tokens in `.npmrc` files:
/// the secret lives in the env file instead of the shell, the motivating
/// registry-token case. The auth-file override stands in for the user-level
/// `.npmrc` so the test stays hermetic (project-level `.npmrc` files
/// deliberately do not expand environment variables).
#[test]
fn npmrc_placeholders_resolve_from_env_file() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".env"),
        "PACQUET_ENV_FILE_TEST_REGISTRY=https://env-file-test.invalid/\n",
    )
    .expect("write .env");
    fs::write(workspace.join("authrc"), "registry=${PACQUET_ENV_FILE_TEST_REGISTRY}\n")
        .expect("write authrc");

    let output = pacquet_in(&workspace)
        .with_args(["--env-file", ".env", "--npmrc-auth-file", "./authrc", "get", "registry"])
        .output()
        .expect("run pacquet get registry");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "command must exit 0, got: {output:?}");
    assert!(
        stdout.trim_end() == "https://env-file-test.invalid/",
        "registry must resolve from the env file; stdout: {stdout:?}",
    );

    drop(root);
}

/// Relative `--env-file` paths resolve against the invocation working
/// directory, before `--dir` is applied.
#[test]
fn relative_env_file_resolves_against_invocation_cwd() {
    let CommandTempCwd { pacquet: _, root, workspace, .. } = CommandTempCwd::init();
    let subdir = workspace.join("subproject");
    fs::create_dir_all(&subdir).expect("create subproject dir");
    write_manifest(&subdir);
    fs::write(workspace.join(".env"), "PACQUET_ENV_FILE_SCRIPT=from-invocation-cwd\n")
        .expect("write caller .env");
    fs::write(subdir.join(".env"), "PACQUET_ENV_FILE_SCRIPT=from-subdir\n")
        .expect("write subdir .env");

    let output = pacquet_in(&workspace)
        .with_args(["--dir", "subproject", "--env-file", ".env", "run", "print-env"])
        .output()
        .expect("run pacquet with --dir and --env-file");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "command must exit 0, got: {output:?}");
    assert!(
        stdout.contains("from-invocation-cwd|"),
        "env file must resolve against the invocation cwd; stdout: {stdout:?}",
    );

    drop(root);
}

/// Dispatching to a pinned older pnpm forwards the loaded variables through
/// the environment and strips `--env-file` from the child argv so the older
/// pnpm does not see the flag.
#[test]
fn dispatched_pinned_pnpm_inherits_env_file_and_strips_flag() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let manifest = serde_json::json!({
        "name": "test",
        "version": "0.0.0",
        "devEngines": {
            "packageManager": {
                "name": "pnpm",
                "version": "9.3.0",
                "onFail": "download",
            },
        },
        "scripts": {
            "print-env": r#"node -e "process.stdout.write([process.env.npm_config_user_agent, process.env.PACQUET_ENV_FILE_SCRIPT, process.env.npm_config_env_file || 'stripped'].join('|'))""#,
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(
        workspace.join(".env"),
        format!(
            "PNPM_CONFIG_REGISTRY={}\nPACQUET_ENV_FILE_SCRIPT=from-env-file\n",
            mock_instance.url(),
        ),
    )
    .expect("write .env");

    let mut command = pacquet.without_ambient_pnpm_config();
    command.env("PNPM_HOME", root.path().join("pnpm-home"));
    command.env("HOME", root.path());
    command.env("XDG_CONFIG_HOME", root.path().join("xdg-config"));
    command.env_remove("COREPACK_ROOT");
    let output = command
        .with_current_dir(&workspace)
        .with_args(["--env-file", ".env", "run", "print-env"])
        .output()
        .expect("run pacquet with pinned pnpm and --env-file");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "command must exit 0, got: {output:?}");
    assert!(
        stdout.contains("pnpm/9.3.0 "),
        "pinned pnpm 9.3.0 must run the script; stdout: {stdout:?}",
    );
    assert!(
        stdout.contains("|from-env-file|stripped"),
        "child pnpm must inherit the env var and not receive --env-file; stdout: {stdout:?}",
    );

    drop((root, mock_instance));
}
