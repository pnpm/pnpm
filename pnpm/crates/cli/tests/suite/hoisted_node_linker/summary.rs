use super::{
    AddMockedRegistry, CommandTempCwd, fs, fs_remove_dir_all, pacquet_at, write_manifest,
    write_workspace_yaml,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use std::process::Command;

/// The reporter's summary block under `header_line`, or `None` when the
/// install printed none.
fn summary_of(stdout: &str, header_line: &str) -> Option<String> {
    let mut lines = stdout
        .lines()
        .skip_while(|line| *line != header_line);
    let header = lines.next()?;
    let entries = lines.take_while(|line| line.starts_with('+') || line.starts_with('-'));
    Some(
        std::iter::once(header)
            .chain(entries)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn install_summary(command: Command) -> Option<String> {
    install_summary_of(command, "dependencies:")
}

fn install_summary_of(command: Command, header_line: &str) -> Option<String> {
    let assert = command
        .with_arg("install")
        .assert()
        .success();
    summary_of(&String::from_utf8_lossy(&assert.get_output().stdout), header_line)
}

/// pnpm/pnpm#15161. The hoisted linker creates no `node_modules/<alias>`
/// symlink, which is what makes every other linker report a direct
/// dependency, so a hoisted install reported none of them.
#[test]
fn hoisted_install_reports_the_version_a_dependency_resolved_to() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");
    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/foo": "^100.0.0" }));

    assert_eq!(install_summary(pacquet).as_deref(), Some("dependencies:\n+ @pnpm.e2e/foo 100.1.0"));

    // Nothing changed, so there is nothing to report.
    assert_eq!(install_summary(pacquet_at(&workspace)), None);

    // The lockfile the previous install left behind goes with
    // `node_modules`, so the next install reports what it puts back.
    fs_remove_dir_all(&workspace.join("node_modules"));
    assert_eq!(
        install_summary(pacquet_at(&workspace)).as_deref(),
        Some("dependencies:\n+ @pnpm.e2e/foo 100.1.0"),
    );

    drop((root, mock_instance));
}

#[test]
fn hoisted_install_reports_both_sides_of_a_version_change() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");
    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/foo": "100.0.0" }));
    install_summary(pacquet);

    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/foo": "100.1.0" }));
    assert_eq!(
        install_summary(pacquet_at(&workspace)).as_deref(),
        Some("dependencies:\n- @pnpm.e2e/foo 100.0.0\n+ @pnpm.e2e/foo 100.1.0"),
    );

    write_manifest(&workspace, serde_json::json!({}));
    assert_eq!(
        install_summary(pacquet_at(&workspace)).as_deref(),
        Some("dependencies:\n- @pnpm.e2e/foo 100.1.0"),
    );

    drop((root, mock_instance));
}

/// The alias is the directory under `node_modules`; the package behind
/// it has its own name, and the summary names both.
#[test]
fn hoisted_install_reports_an_aliased_dependency_under_its_alias() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");
    write_manifest(&workspace, serde_json::json!({ "aliased": "npm:@pnpm.e2e/foo@100.1.0" }));

    assert_eq!(
        install_summary(pacquet).as_deref(),
        Some("dependencies:\n+ aliased <- @pnpm.e2e/foo 100.1.0"),
    );

    drop((root, mock_instance));
}

/// The install resolves an unsupported optional dependency and leaves it
/// uninstalled, so the summary must not claim otherwise.
#[test]
fn hoisted_install_does_not_report_an_optional_dependency_it_skipped() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/not-compatible-with-any-os": "*" },
        })
        .to_string(),
    )
    .expect("write package.json");

    assert_eq!(install_summary(pacquet), None);
    assert!(!workspace.join("node_modules/@pnpm.e2e/not-compatible-with-any-os").exists());

    drop((root, mock_instance));
}

/// A dependency this install skips but the last one installed has been
/// taken off disk, so the summary has to say so.
#[test]
fn hoisted_install_reports_an_optional_dependency_it_stops_supporting() {
    const PKG: &str = "@pnpm.e2e/not-compatible-with-any-os";
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "optionalDependencies": { PKG: "*" } }).to_string(),
    )
    .expect("write package.json");

    write_workspace_yaml(
        &workspace,
        "nodeLinker: hoisted\nsupportedArchitectures:\n  os:\n    - this-os-does-not-exist\n",
    );
    assert_eq!(
        install_summary_of(pacquet, "optionalDependencies:").as_deref(),
        Some(&*format!("optionalDependencies:\n+ {PKG} 1.0.0")),
    );

    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");
    assert_eq!(
        install_summary_of(pacquet_at(&workspace), "optionalDependencies:").as_deref(),
        Some(&*format!("optionalDependencies:\n- {PKG} 1.0.0")),
    );
    let installed = workspace.join("node_modules").join(PKG);
    assert!(!installed.exists(), "the unsupported package must be gone: {installed:?}");

    drop((root, mock_instance));
}
