// `assert_cmd::prelude::*` (for `.assert()`) is only used by the Unix-
// gated dlx happy-path test below; gating the import avoids an
// `unused_imports` error on Windows under clippy's `-D warnings`.
#[cfg(unix)]
use assert_cmd::prelude::*;
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
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

        let mut command = pacquet;
        if let Some(reporter) = reporter {
            command.arg(reporter);
        }
        command.arg("dlx");
        let output = command.output().expect("spawn pacquet dlx");
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("STDERR:\n{stderr}\n");
        assert!(!output.status.success(), "dlx with no command must fail");
        assert!(
            stderr.contains("requires a command to run"),
            "the failure must be the missing-command diagnostic",
        );

        drop(root);
    }
}

/// `pacquet dlx <package>` resolves the package against the mocked
/// registry, installs it into the dlx cache under `config.cache_dir`,
/// and runs its bin in the process cwd. Mirrors pnpm's `dlx` happy
/// path (dlx.ts). Uses `@foo/touch-file-one-bin`, whose single bin
/// writes `touch.txt` when invoked — the file's presence in cwd
/// proves both the install and the bin execution worked end-to-end.
///
/// Locally this needs the in-repo pnpr (the mocked registry); in CI
/// `add_mocked_registry()` starts it via `pacquet-testing-utils`.
#[cfg(unix)]
#[test]
fn dlx_installs_and_runs_packages_bin() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.with_arg("dlx").with_arg("@foo/touch-file-one-bin").assert().success();

    assert!(
        workspace.join("touch.txt").exists(),
        "the package's bin should run in the process cwd and write `touch.txt`",
    );

    drop(root);
}
