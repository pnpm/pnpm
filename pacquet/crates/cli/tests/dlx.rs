use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;

/// `pacquet dlx` with no command is an error, mirroring pnpm's dlx, which
/// prints help and exits non-zero when given neither a command nor a
/// `--package`.
///
/// The happy path (resolve, install into the cache, run the bin) needs
/// the mocked registry and is exercised in CI rather than here.
#[test]
fn dlx_errors_when_no_command_given() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_arg("dlx").output().expect("spawn pacquet dlx");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success(), "dlx with no command must fail");
    assert!(
        stderr.contains("requires a command to run"),
        "the failure must be the missing-command diagnostic",
    );

    drop(root);
}
