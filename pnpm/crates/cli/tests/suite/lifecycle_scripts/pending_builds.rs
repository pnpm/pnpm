use super::workspace_yaml::{allow_builds, append_workspace_yaml_key};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_modules_yaml::{Host, read_modules_manifest};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

fn read_pending_builds(workspace: &Path) -> Vec<String> {
    read_modules_manifest::<Host>(&workspace.join("node_modules"))
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists")
        .pending_builds
}

/// TS: `run pre/postinstall scripts` (`deps-restorer/test/index.ts:362`),
/// the `ignoreScripts` tail. Ordering is pnpm's: projects first.
#[test]
fn ignore_scripts_records_the_project_and_its_deferred_dependency() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "scripts": { "install": r#"node -e """#, "postinstall": r#"node -e """# },
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();

    assert_eq!(
        read_pending_builds(&workspace),
        [".", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
    );

    drop((root, mock_instance));
}

/// A project without install-time scripts of its own contributes no
/// importer entry — only the deferred dependency builds are recorded.
#[test]
fn a_project_without_install_scripts_records_only_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();

    assert_eq!(
        read_pending_builds(&workspace),
        ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
    );

    drop((root, mock_instance));
}

/// An install that runs the build scripts owes nothing, so the list
/// stays empty — this is what makes a populated list mean "deferred".
#[test]
fn an_install_that_runs_the_scripts_records_nothing() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

    pacquet.with_arg("install").assert().success();

    assert_eq!(read_pending_builds(&workspace), Vec::<String>::new());

    drop((root, mock_instance));
}

/// TS: `pendingBuilds gets updated if install removes packages`
/// (`deps-installer/test/lockfile.ts:614`). Dropping one of two
/// deferred dependencies shrinks the recorded list.
#[test]
fn removing_a_package_shrinks_the_list() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let both = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            "@pnpm.e2e/install-script-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, both.to_string()).expect("write package.json");

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
    let before = read_pending_builds(&workspace);
    assert_eq!(
        before,
        [
            "@pnpm.e2e/install-script-example@1.0.0",
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0",
        ],
    );

    let one = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(&manifest_path, one.to_string()).expect("rewrite package.json");

    let CommandTempCwd { pacquet: rerun, root: rerun_root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    rerun
        .with_current_dir(&workspace)
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();

    let after = read_pending_builds(&workspace);
    assert_eq!(after, ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"]);
    assert!(after.len() < before.len(), "removing a package must shrink {before:?}");

    drop((root, mock_instance, rerun_root));
}

/// `pnpm rebuild --pending` is what settles the debt: it runs both
/// the deferred dependency builds and the project's own deferred
/// install scripts, then leaves the list empty so a second run has
/// nothing to do.
#[test]
fn rebuild_pending_runs_the_deferred_work_and_empties_the_list() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let marker = workspace.join("project-install-ran.txt");
    let package_json = serde_json::json!({
        "scripts": {
            "install": r#"node -e "require('fs').writeFileSync('project-install-ran.txt','ran')""#,
        },
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
    assert_eq!(
        read_pending_builds(&workspace),
        [".", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
    );
    assert!(!marker.exists(), "--ignore-scripts must defer the project's own script");
    append_workspace_yaml_key(&workspace, "pending", true);

    let CommandTempCwd { pacquet: rebuild, root: rebuild_root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    rebuild.with_current_dir(&workspace).arg("rebuild").assert().success();

    assert!(marker.exists(), "the deferred project script must run");
    assert!(
        workspace
            .join(
                "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
                     /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example\
                     /generated-by-postinstall.js"
            )
            .exists(),
        "the deferred dependency build must run",
    );
    assert_eq!(
        read_pending_builds(&workspace),
        Vec::<String>::new(),
        "a rebuild settles the debt it was asked to run",
    );

    drop((root, mock_instance, rebuild_root));
}

/// A `rebuild --pending` that the `allowBuilds` policy still blocks
/// runs nothing, so it must leave the debt in place — dropping it
/// would let a later `--pending` report success on a build that never
/// ran.
#[test]
fn rebuild_pending_keeps_a_dependency_the_policy_still_blocks() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
    let pending = read_pending_builds(&workspace);
    assert_eq!(pending, ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"]);

    // No `allowBuilds` entry, so `rebuild --pending` cannot build it.
    // Its exit code is beside the point — the assertion is that the
    // debt survives whether it exits 0 (warn) or 1 (strict).
    let CommandTempCwd { pacquet: rebuild, root: rebuild_root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let _ = rebuild
        .with_current_dir(&workspace)
        .with_args(["rebuild", "--pending"])
        .output()
        .expect("run pacquet rebuild --pending");

    assert_eq!(read_pending_builds(&workspace), pending, "a build the policy blocked stays owed");

    drop((root, mock_instance, rebuild_root));
}

/// A project's scripts run after `.modules.yaml` is written, so its
/// entry can only be cleared once they have actually succeeded —
/// otherwise a failing script would lose the debt it just failed to
/// discharge.
#[test]
fn a_failed_project_script_keeps_its_pending_entry() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "scripts": { "install": r#"node -e "process.exit(1)""# },
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
    assert_eq!(
        read_pending_builds(&workspace),
        [".", "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"],
    );

    let CommandTempCwd { pacquet: rebuild, root: rebuild_root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let output = rebuild
        .with_current_dir(&workspace)
        .with_args(["rebuild", "--pending"])
        .output()
        .expect("run pacquet rebuild --pending");
    eprintln!(
        "rebuild --pending output:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.status.success(), "a failing project script must fail the rebuild");

    assert_eq!(
        read_pending_builds(&workspace),
        ["."],
        "the dependency was rebuilt, the project was not",
    );

    drop((root, mock_instance, rebuild_root));
}

/// A build stays owed until something runs it: an install that does
/// not defer anything new must not drop what an earlier
/// `--ignore-scripts` install recorded.
#[test]
fn a_later_install_preserves_what_an_earlier_one_deferred() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    // Approved, so the second install's build phase reports nothing
    // deferred — the entry can only survive by being carried over.
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);

    pacquet.with_args(["install", "--ignore-scripts"]).assert().success();
    let deferred = read_pending_builds(&workspace);
    assert_eq!(deferred, ["@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0"]);

    let CommandTempCwd { pacquet: rerun, root: rerun_root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    rerun.with_current_dir(&workspace).with_arg("install").assert().success();

    assert_eq!(read_pending_builds(&workspace), deferred);

    drop((root, mock_instance, rerun_root));
}
