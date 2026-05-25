use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};

/// `pacquet dlx <pkg>` installs the package into the dlx cache and runs
/// its single binary. Uses the mocked registry's
/// `@pnpm.e2e/hello-world-js-bin`, whose bin prints to stdout.
#[cfg(unix)]
#[test]
fn dlx_installs_and_runs_a_package_bin() {
    let CommandTempCwd { pacquet, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet
        .with_arg("dlx")
        .with_arg("@pnpm.e2e/hello-world-js-bin")
        .output()
        .expect("spawn pacquet dlx");

    assert!(
        output.status.success(),
        "dlx should install and run the package bin\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !output.stdout.is_empty(),
        "the bin should have produced output; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    drop((root, mock_instance));
}

/// `pacquet dlx` with neither a command nor `--package` is an error.
#[test]
fn dlx_requires_a_command_or_package() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let output = pacquet.with_arg("dlx").output().expect("spawn pacquet dlx");
    assert!(!output.status.success(), "bare dlx must fail");
    drop(root);
}
