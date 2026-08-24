use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;

/// pnpm registers the npm commands it has not implemented so they name the
/// npm CLI. Without that they would reach the external-subcommand
/// fallback and fail as a missing script instead.
#[test]
fn the_unimplemented_npm_commands_point_at_the_npm_cli() {
    for command in ["edit", "profile", "token", "xmas"] {
        let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
        let output = pacquet
            .with_args([command])
            .output()
            .unwrap_or_else(|error| panic!("run pacquet {command}: {error}"));
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{command} stderr={stderr}");
        assert!(!output.status.success(), "pacquet {command} must fail");
        assert!(stderr.contains("ERR_PNPM_NOT_IMPLEMENTED"), "{command}: {stderr}");
        assert!(stderr.contains(&format!("npm {command}")), "{command}: {stderr}");

        drop(root);
    }
}

/// The arguments an npm invocation would carry are swallowed, so the user
/// sees the same pointer rather than an argument-parsing error.
#[test]
fn the_unimplemented_commands_swallow_their_arguments() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let output = pacquet
        .with_args(["token", "create", "--read-only"])
        .output()
        .expect("run pacquet token create");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("stderr={stderr}");
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_NOT_IMPLEMENTED"), "{stderr}");

    drop(root);
}
