#[cfg(unix)]
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path};

/// `pacquet create` with no template name is an error, mirroring pnpm's
/// `create`, which throws `ERR_PNPM_MISSING_ARGS` when given no arguments.
#[test]
fn create_errors_when_no_name_given() {
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

        let mut command = pacquet;
        if let Some(reporter) = reporter {
            command.arg(reporter);
        }
        command.arg("create");
        let output = command.output().expect("spawn pacquet create");
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("STDERR:\n{stderr}\n");
        assert!(!output.status.success(), "create with no name must fail");
        assert!(
            stderr.contains("ERR_PNPM_MISSING_ARGS"),
            "stderr must contain the pnpm-compatible error code",
        );
        assert!(
            stderr.contains("Missing the template package name"),
            "stderr must contain the human-readable message",
        );

        drop(root);
    }
}

/// `pacquet create <name>` converts the name to `create-<name>` and
/// delegates to dlx. Uses the mocked registry with a test package
/// whose bin writes a file on execution.
#[cfg(unix)]
#[test]
fn create_converts_name_and_runs_via_dlx() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let output = pacquet
        .with_args(["--reporter=append-only", "create", "touch-file-one-bin"])
        .output()
        .expect("run pacquet create");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "create failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    // The reporter writes to stderr for create (pnpm's
    // `COMMANDS_WITH_STDERR_REPORTER`), keeping stdout for the executed
    // command.
    assert!(
        stderr.contains("dependencies:\n+ create-touch-file-one-bin"),
        "create should print the installed package summary on stderr\nstderr:\n{stderr}",
    );

    let touch_txt = workspace.join("touch.txt");
    assert!(
        touch_txt.exists(),
        "the package's bin should run in the process cwd and write `touch.txt`",
    );
    let content = std::fs::read_to_string(&touch_txt).unwrap();
    assert_eq!(content, "[]", "no extra arguments should be forwarded");

    drop(root);
}

/// `pacquet create` passes remaining arguments through to the package.
#[cfg(unix)]
#[test]
fn create_forwards_args_to_package() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_arg("create")
        .with_arg("touch-file-one-bin")
        .with_arg("--extra-arg")
        .assert()
        .success();

    let touch_txt = workspace.join("touch.txt");
    assert!(touch_txt.exists(), "touch.txt must exist");
    let content = std::fs::read_to_string(&touch_txt).unwrap();
    assert_eq!(content, r#"["--extra-arg"]"#, "extra argument should be forwarded to the package");

    drop(root);
}

/// `pacquet create --allow-build <name>` passes the flag to dlx.
/// Options must precede the `<name>` positional to be parsed by create;
/// anything after `<name>` is forwarded opaquely (matching pnpm's
/// `escapeArgs` semantics).
#[cfg(unix)]
#[test]
fn create_allow_build_before_name_is_parsed() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_arg("create")
        .with_arg("--allow-build=touch-file-one-bin")
        .with_arg("touch-file-one-bin")
        .assert()
        .success();

    let touch_txt = workspace.join("touch.txt");
    assert!(touch_txt.exists(), "the package should install and run with --allow-build");
    let content = std::fs::read_to_string(&touch_txt).unwrap();
    assert_eq!(
        content, "[]",
        "--allow-build should be parsed/consumed by the CLI and not forwarded",
    );

    drop(root);
}

/// Options placed after `<name>` are forwarded to the package, not parsed
/// by create — matching pnpm's `escapeArgs` behavior where everything
/// after the first positional is opaque.
#[cfg(unix)]
#[test]
fn create_options_after_name_are_forwarded() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_arg("create")
        .with_arg("touch-file-one-bin")
        .with_arg("--allow-build=touch-file-one-bin")
        .assert()
        .success();

    let touch_txt = workspace.join("touch.txt");
    assert!(touch_txt.exists(), "touch.txt must exist");
    let content = std::fs::read_to_string(&touch_txt).unwrap();
    assert_eq!(
        content, r#"["--allow-build=touch-file-one-bin"]"#,
        "options after name should be forwarded to the package",
    );

    drop(root);
}

/// `pacquet create` with `-c` (shell mode) flag works.
#[cfg(unix)]
#[test]
fn create_accepts_shell_mode_flag() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_arg("create")
        .with_arg("-c")
        .with_arg("touch-file-one-bin")
        .assert()
        .success();

    let touch_txt = workspace.join("touch.txt");
    assert!(touch_txt.exists(), "the package should install and run with shell mode");
    let content = std::fs::read_to_string(&touch_txt).unwrap();
    assert_eq!(
        content, "[]",
        "shell mode flag should be parsed/consumed by the CLI and not forwarded",
    );

    drop(root);
}

/// Add `templates/*` and `packages/*` to the workspace, with a `packages/app`
/// project to run `create` from, and a template project named `name` whose
/// bin writes its arguments to `local.txt` in the working directory.
fn add_workspace_template(workspace: &Path, name: &str) {
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut text = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    text.push_str("packages:\n  - templates/*\n  - packages/*\n");
    fs::write(&workspace_yaml, text).expect("write pnpm-workspace.yaml");

    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create packages/app");
    fs::write(app.join("package.json"), r#"{ "name": "app" }"#).expect("write app manifest");

    let template = workspace.join("templates/template");
    fs::create_dir_all(&template).expect("create the template project");
    let manifest = serde_json::json!({ "name": name, "version": "1.0.0", "bin": "cli.js" });
    fs::write(template.join("package.json"), manifest.to_string())
        .expect("write template manifest");
    fs::write(
        template.join("cli.js"),
        "#!/usr/bin/env node\nrequire('fs').writeFileSync('local.txt', JSON.stringify(process.argv.slice(2)))\n",
    )
    .expect("write template bin");
}

/// Inside a workspace, `pacquet create <name>` runs the workspace project
/// named `create-<name>` rather than the registry package, in the process
/// cwd and with the remaining arguments forwarded.
#[test]
fn create_runs_workspace_template() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    add_workspace_template(&workspace, "create-local-kit");
    let app = workspace.join("packages/app");

    let output = pacquet
        .with_current_dir(&app)
        .with_args(["create", "local-kit", "--extra-arg"])
        .output()
        .expect("run pacquet create");
    assert!(
        output.status.success(),
        "create failed\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let content = fs::read_to_string(app.join("local.txt")).expect("the template bin should run");
    assert_eq!(content, r#"["--extra-arg"]"#);

    drop(root);
}

/// `pacquet create @scope/<name>` runs the workspace project named
/// `@scope/create-<name>`.
#[test]
fn create_runs_scoped_workspace_template() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    add_workspace_template(&workspace, "@local/create-kit");

    let output = pacquet
        .with_args(["create", "@local/kit"])
        .output()
        .expect("run pacquet create");
    assert!(
        output.status.success(),
        "create failed\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let content =
        fs::read_to_string(workspace.join("local.txt")).expect("the template bin should run");
    assert_eq!(content, "[]");

    drop(root);
}

/// A name with a version asks for the registry package even when a
/// workspace project has the same name.
#[cfg(unix)]
#[test]
fn create_with_version_ignores_workspace_template() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    add_workspace_template(&workspace, "create-touch-file-one-bin");

    pacquet
        .with_args(["create", "touch-file-one-bin@1.0.0"])
        .assert()
        .success();

    assert!(workspace.join("touch.txt").exists(), "the registry package should run");
    assert!(!workspace.join("local.txt").exists(), "the workspace template must not run");

    drop(root);
}
