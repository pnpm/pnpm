use pacquet_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn runtime_unknown_subcommand_runs_with_default_ndjson_and_silent_reporters() {
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        fs::write(workspace.join("package.json"), "{}").expect("write package.json");

        let mut command = pacquet;
        if let Some(reporter) = reporter {
            command.arg(reporter);
        }
        command.arg("runtime").arg("unknown");
        let output = command.output().expect("spawn pacquet runtime");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "unknown runtime subcommand must fail");
        assert!(stderr.contains("ERR_PNPM_RUNTIME_UNKNOWN_SUBCOMMAND"), "stderr: {stderr}");

        drop(root);
    }
}
