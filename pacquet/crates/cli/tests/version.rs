use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;

#[test]
fn version_flag_prints_the_bare_version() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_arg("--version").output().expect("run pacquet --version");
    dbg!(&output);
    assert!(output.status.success(), "pacquet --version should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", pacquet_config::PACQUET_VERSION),
    );

    drop(root);
}

#[test]
fn short_version_flag_prints_the_bare_version() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_arg("-v").output().expect("run pacquet -v");
    dbg!(&output);
    assert!(output.status.success(), "pacquet -v should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", pacquet_config::PACQUET_VERSION),
    );

    drop(root);
}
