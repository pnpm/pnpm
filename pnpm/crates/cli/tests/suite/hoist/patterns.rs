use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, fs, generate_lockfile, is_symlink_or_junction,
    pacquet_in, write_manifest, write_workspace_yaml,
};
use assert_cmd::assert::OutputAssertExt;

/// Default hoist patterns hoist every transitive into
/// `<vs>/node_modules/`.
/// Single-importer subset — the persisted map surviving a repeat
/// install is covered by
/// [`should_hoist_dependencies_repeat_install_preserves_map`].
#[test]
fn private_hoist_default_pattern_hoists_transitives() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(
        is_symlink_or_junction(&workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent"))
            .unwrap(),
        "direct dep symlink missing",
    );
    let private_hoist =
        workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&private_hoist).unwrap(),
        "transitive `@pnpm.e2e/hello-world-js-bin` should be hoisted to {private_hoist:?}",
    );
    // Public-hoist patterns default to `[]` (matching pnpm v11), so
    // no transitive can match — it should NOT be at the root.
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "transitive should not be publicly hoisted under default patterns",
    );

    drop((root, mock_instance));
}

/// Both patterns empty → no hoist symlinks anywhere. An empty pattern
/// list compiles to a never-matches matcher, so the hoist pass still
/// runs (the `is_some()` guard sees `Some([])`); it just produces no
/// entries.
#[test]
fn both_patterns_empty_produces_no_hoist_symlinks() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(&workspace, "hoistPattern: []\npublicHoistPattern: []\n");

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(
        is_symlink_or_junction(&workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent"))
            .unwrap(),
    );
    assert!(
        !workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "no private hoist with empty patterns",
    );
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "no public hoist with empty patterns",
    );

    drop((root, mock_instance));
}

/// `shamefullyHoist: true` is the legacy alias for
/// `publicHoistPattern: ["*"]`, translated in
/// the final merged config.
#[test]
fn shamefully_hoist_legacy_publicly_hoists_everything() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(&workspace, "shamefullyHoist: true\n");

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let public_hoist = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&public_hoist).unwrap(),
        "shamefullyHoist should publicly hoist everything",
    );

    drop((root, mock_instance));
}

#[test]
fn shamefully_hoist_cli_option_publicly_hoists_everything() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);

    pacquet
        .with_args(["--shamefully-hoist=true", "install", "--frozen-lockfile"])
        .assert()
        .success();

    let public_hoist = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&public_hoist).unwrap(),
        "--shamefully-hoist should publicly hoist everything at {public_hoist:?}",
    );

    drop((root, mock_instance));
}

/// Regression for [pnpm/pnpm#11750](https://github.com/pnpm/pnpm/issues/11750):
/// pacquet's default `publicHoistPattern` must match pnpm v11's
/// (empty list) so a follow-up `pnpm` invocation in the same project
/// doesn't reject the `.modules.yaml` with
/// `ERR_PNPM_PUBLIC_HOIST_PATTERN_DIFF`.
#[test]
fn modules_yaml_public_hoist_pattern_matches_pnpm_default() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let modules_yaml_text = fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
        .expect("read .modules.yaml");
    assert!(
        modules_yaml_text.contains(r#""publicHoistPattern": []"#),
        "publicHoistPattern should serialize as an empty list (pnpm default); got:\n{modules_yaml_text}",
    );

    drop((root, mock_instance));
}

/// `hoistPattern: ["@pnpm.e2e/*"]` — only aliases under the
/// `@pnpm.e2e` scope hoist privately. (Uses the `@pnpm.e2e` package
/// set; the registry mock doesn't carry the `express` family.)
#[test]
fn private_hoist_pattern_filters_aliases() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(&workspace, "hoistPattern:\n  - '@pnpm.e2e/*'\npublicHoistPattern: []\n");

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let private_hoist =
        workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&private_hoist).unwrap(),
        "scoped pattern should hoist `@pnpm.e2e/hello-world-js-bin`",
    );

    drop((root, mock_instance));
}

/// `!`-negation excludes a specific alias from hoisting. Pattern
/// `["*", "!@pnpm.e2e/hello-world-js-bin"]` — everything except this
/// one alias. Mirrors the matcher's negation semantics integration-
/// test-side; the matcher itself has unit coverage in
/// `crates/config/src/matcher.rs`.
#[test]
fn negation_pattern_excludes_alias_from_hoist() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(
        &workspace,
        "hoistPattern:\n  - '*'\n  - '!@pnpm.e2e/hello-world-js-bin'\npublicHoistPattern: []\n",
    );

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let private_hoist =
        workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        !private_hoist.exists(),
        "negation pattern should exclude `@pnpm.e2e/hello-world-js-bin` from hoist; \
         found at {private_hoist:?}",
    );

    drop((root, mock_instance));
}

/// TS: `hoistPattern=* throws exception when executed on node_modules
/// installed w/o the option` (`hoist.ts:209`): `add` refuses to touch a
/// modules dir whose persisted hoist pattern disagrees.
#[test]
fn hoist_pattern_mismatch_throws_against_existing_modules_yaml() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace_yaml(&workspace, "hoistPattern: []\n");
    pacquet.with_args(["add", "is-positive@1.0.0"]).assert().success();

    write_workspace_yaml(&workspace, "");
    let output = pacquet_in(&workspace).with_args(["add", "is-negative@1.0.0"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_HOIST_PATTERN_DIFF"),
        "expected the hoist-pattern diff error, got: {stderr}",
    );

    drop((root, mock_instance));
}

/// TS: `hoistPattern=undefined throws exception when executed on
/// node_modules installed with hoist-pattern=*` (`hoist.ts:220`) — the
/// mirror of the test above.
#[test]
fn hoist_pattern_undefined_throws_against_hoisted_modules_yaml() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet.with_args(["add", "is-positive@1.0.0"]).assert().success();

    write_workspace_yaml(&workspace, "hoistPattern: []\n");
    let output = pacquet_in(&workspace).with_args(["add", "is-negative@1.0.0"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_HOIST_PATTERN_DIFF"),
        "expected the hoist-pattern diff error, got: {stderr}",
    );

    drop((root, mock_instance));
}

/// TS: `should hoist some dependencies to the root of node_modules when
/// publicHoistPattern is used and others to the virtual store directory`
/// (`hoist.ts:89`), on registry-mock fixtures: the public pattern's
/// matches land in root `node_modules`, everything else goes private.
#[test]
fn combined_public_and_private_hoist_patterns_split_targets() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace_yaml(
        &workspace,
        "publicHoistPattern:\n  - '*dep-of-pkg-with-1-dep*'\nhoistPattern:\n  - '*'\n",
    );
    write_manifest(
        &workspace,
        serde_json::json!({
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            "@pnpm.e2e/foobarqar": "1.0.0",
        }),
    );
    pacquet.with_arg("install").assert().success();

    assert!(
        workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep").exists(),
        "the public pattern's match must land in root node_modules",
    );
    for name in ["foo", "bar"] {
        assert!(
            workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e").join(name).exists(),
            "{name} must be privately hoisted",
        );
        assert!(
            !workspace.join("node_modules/@pnpm.e2e").join(name).exists(),
            "{name} must not be publicly hoisted",
        );
    }

    drop((root, mock_instance));
}
