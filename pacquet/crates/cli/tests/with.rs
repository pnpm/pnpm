//! `pacquet with <version|current> <args...>` runs pnpm at a specific
//! version (or the currently running one) for a single invocation,
//! ignoring the project's `packageManager` / `devEngines.packageManager`
//! pin.
//!
//! These cover the network-free surface of the command: the usage errors
//! and the `with current` argv rewrite. The end-to-end `with <version>`
//! download path is not covered here because the mock registry does not
//! serve the `pnpm` / `@pnpm/exe` engine packages (the same reason
//! `self-update` has no end-to-end install test).

use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;

#[test]
fn with_requires_a_version_spec() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_args(["with"]).output().expect("run pacquet with");
    dbg!(&output);
    assert!(!output.status.success(), "pacquet with (no spec) should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Missing version argument"), "stderr should explain the gap: {stderr}");

    drop(root);
}

#[test]
fn with_current_requires_a_command() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_args(["with", "current"]).output().expect("run pacquet with current");
    dbg!(&output);
    assert!(!output.status.success(), "pacquet with current (no command) should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(r#"Missing command after "current""#),
        "stderr should explain the gap: {stderr}"
    );

    drop(root);
}

#[test]
fn with_current_runs_the_inner_command() {
    // `with current <cmd>` is rewritten to a direct `<cmd>` dispatch, so an
    // unknown inner command surfaces clap's error for that command — proof
    // the `with current` tokens were stripped and `<cmd>` reached the
    // parser, rather than being swallowed by the `with` subcommand.
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["with", "current", "this-command-does-not-exist"])
        .output()
        .expect("run pacquet with current <cmd>");
    dbg!(&output);
    assert!(!output.status.success(), "an unknown inner command should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("this-command-does-not-exist") || stderr.contains("unrecognized"),
        "the inner command should reach the parser: {stderr}"
    );

    drop(root);
}
