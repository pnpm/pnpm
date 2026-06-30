#[cfg(unix)]
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;

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

    pacquet.with_arg("create").with_arg("touch-file-one-bin").assert().success();

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

    pacquet.with_arg("create").with_arg("-c").with_arg("touch-file-one-bin").assert().success();

    let touch_txt = workspace.join("touch.txt");
    assert!(touch_txt.exists(), "the package should install and run with shell mode");
    let content = std::fs::read_to_string(&touch_txt).unwrap();
    assert_eq!(
        content, "[]",
        "shell mode flag should be parsed/consumed by the CLI and not forwarded",
    );

    drop(root);
}
