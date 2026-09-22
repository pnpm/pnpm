use super::{
    CommandTempCwd,
    EXEC_NODE_VERSION,
    assert_contains,
    assert_failure,
    assert_success,
    output_text,
    run,
    stderr,
    stdout,
    write_runtime,
};

#[test]
fn dev_engines_runtime_with_on_fail_error_reports_a_node_version_mismatch() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(&stderr(&output), "This project requires Node.js 99999.0.0");
}

#[test]
fn dev_engines_runtime_with_on_fail_warn_warns_about_a_node_version_mismatch() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "warn",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_success(&output);
    assert_contains(&output_text(&output), "This project requires Node.js 99999.0.0");
}

#[test]
fn dev_engines_runtime_with_on_fail_ignore_is_not_checked() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "ignore",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_success(&output);
    assert!(!output_text(&output).contains("99999.0.0"), "unexpected mention of the pinned range");
}

#[test]
fn engines_runtime_is_checked_too() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "engines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(&stderr(&output), "This project requires Node.js 99999.0.0");
}

#[test]
fn an_invalid_node_version_range_fails_with_the_runtime_on_fail_hint() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "invalid range", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    let stderr = stderr(&output);
    assert_contains(
        &stderr,
        "This project requires an invalid Node.js version range: invalid range",
    );
    assert_contains(&stderr, "--runtime-on-fail=ignore");
}

#[test]
fn an_invalid_deno_version_range_names_deno() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "deno", "version": "invalid range", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(
        &stderr(&output),
        "This project requires an invalid Deno version range: invalid range",
    );
}

#[test]
fn an_invalid_bun_version_range_names_bun() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "bun", "version": "invalid range", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(
        &stderr(&output),
        "This project requires an invalid Bun version range: invalid range",
    );
}

#[test]
fn a_runtime_without_a_version_range_fails() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(
        &stderr(&output),
        "This project requires a Node.js runtime but does not specify a version range",
    );
}

#[test]
fn runtime_array_entries_are_checked_beyond_the_first_one() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!([
            { "name": "node", "version": "*", "onFail": "error" },
            { "name": "deno", "version": "invalid range", "onFail": "error" },
        ]),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(
        &stderr(&output),
        "This project requires an invalid Deno version range: invalid range",
    );
}

#[test]
fn runtime_on_fail_ignore_bypasses_the_manifest_on_fail() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "error",
        }),
    );

    let output = run(
        pacquet,
        root.path(),
        &[
            "--config.verify-deps-before-run=false",
            "--config.runtime-on-fail=ignore",
            "exec",
            "node",
            "--version",
        ],
    );

    assert_success(&output);
    assert!(!output_text(&output).contains("99999.0.0"), "unexpected mention of the pinned range");
}

#[test]
fn a_failing_runtime_check_does_not_block_the_version_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &["--version"]);

    assert_success(&output);
    assert_eq!(stdout(&output).trim(), pnpm_config::PNPM_VERSION);
}
