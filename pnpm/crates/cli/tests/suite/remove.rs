use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn manifest_has(workspace: &Path, group: DependencyGroup, name: &str) -> bool {
    PackageManifest::from_path(workspace.join("package.json"))
        .expect("read package.json")
        .dependencies([group])
        .any(|(key, _)| key == name)
}

#[test]
fn should_remove_from_package_json() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet
        .with_args(["add", "@pnpm.e2e/hello-world-js-bin"])
        .assert()
        .success();
    assert!(manifest_has(&workspace, DependencyGroup::Prod, "@pnpm.e2e/hello-world-js-bin"));

    pacquet_at(&workspace)
        .with_args(["remove", "@pnpm.e2e/hello-world-js-bin"])
        .assert()
        .success();

    eprintln!("the dependency is gone from package.json#dependencies");
    assert!(!manifest_has(&workspace, DependencyGroup::Prod, "@pnpm.e2e/hello-world-js-bin"));

    drop((root, mock_instance));
}

#[test]
fn remove_runs_with_ndjson_and_silent_reporters() {
    for reporter in ["--reporter=ndjson", "--reporter=silent"] {
        let CommandTempCwd {
            pacquet,
            root,
            workspace,
            npmrc_info,
            ..
        } = CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        pacquet
            .with_args(["add", "@pnpm.e2e/hello-world-js-bin"])
            .assert()
            .success();
        assert!(manifest_has(&workspace, DependencyGroup::Prod, "@pnpm.e2e/hello-world-js-bin"));

        pacquet_at(&workspace)
            .with_args([reporter, "remove", "@pnpm.e2e/hello-world-js-bin"])
            .assert()
            .success();
        assert!(!manifest_has(&workspace, DependencyGroup::Prod, "@pnpm.e2e/hello-world-js-bin"));

        drop((root, mock_instance));
    }
}

#[test]
fn should_remove_only_from_targeted_field() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet
        .with_args(["add", "@pnpm.e2e/hello-world-js-bin", "--save-dev"])
        .assert()
        .success();
    assert!(manifest_has(&workspace, DependencyGroup::Dev, "@pnpm.e2e/hello-world-js-bin"));

    eprintln!("`remove --save-prod` must not touch a devDependency");
    let output = pacquet_at(&workspace)
        .with_args(["remove", "@pnpm.e2e/hello-world-js-bin", "--save-prod"])
        .output()
        .expect("spawn pacquet remove");
    assert!(
        !output.status.success(),
        "removing from the wrong field must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS"),
        "stderr must name the missing-deps diagnostic; got:\n{stderr}",
    );
    assert!(manifest_has(&workspace, DependencyGroup::Dev, "@pnpm.e2e/hello-world-js-bin"));

    eprintln!("`remove --save-dev` removes it");
    pacquet_at(&workspace)
        .with_args(["remove", "@pnpm.e2e/hello-world-js-bin", "--save-dev"])
        .assert()
        .success();
    assert!(!manifest_has(&workspace, DependencyGroup::Dev, "@pnpm.e2e/hello-world-js-bin"));

    drop((root, mock_instance));
}

#[test]
fn should_fail_when_no_package_specified() {
    let CommandTempCwd { pacquet, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet
        .with_args(["remove"])
        .output()
        .expect("spawn pacquet remove");
    assert!(
        !output.status.success(),
        "remove with no names must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_MUST_REMOVE_SOMETHING"),
        "stderr must name the must-remove-something diagnostic; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn should_fail_when_dependency_is_missing() {
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
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_args(["remove", "is-negative"])
        .output()
        .expect("spawn pacquet remove");
    assert!(
        !output.status.success(),
        "removing an absent dependency must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS"),
        "stderr must name the missing-deps diagnostic; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn should_report_project_has_no_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // A manifest with no dependency fields at all, removed without a
    // `--save-*` flag, hits the "no dependencies of any kind" branch —
    // a missing-deps error with no hint.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "fixture", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_args(["remove", "is-positive"])
        .output()
        .expect("spawn pacquet remove");
    assert!(
        !output.status.success(),
        "removing from an empty manifest must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS"),
        "stderr must name the missing-deps diagnostic; got:\n{stderr}",
    );
    assert!(
        stderr.contains("project has no dependencies of any kind"),
        "stderr must explain the project has no dependencies; got:\n{stderr}",
    );
    assert!(
        !stderr.contains("Available dependencies"),
        "no hint should be emitted when there are no available dependencies; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn should_accept_aliases() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet
        .with_args(["add", "@pnpm.e2e/hello-world-js-bin"])
        .assert()
        .success();

    eprintln!("`rm` is an alias of `remove`");
    pacquet_at(&workspace)
        .with_args(["rm", "@pnpm.e2e/hello-world-js-bin"])
        .assert()
        .success();
    assert!(!manifest_has(&workspace, DependencyGroup::Prod, "@pnpm.e2e/hello-world-js-bin"));

    drop((root, mock_instance));
}

#[test]
fn recursive_remove_validates_selected_dependencies_before_writing() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    prepare_removal_workspace(&workspace, ["dependencies", "devDependencies"]);
    let files = ["project-1/package.json", "project-2/package.json", "pnpm-workspace.yaml"];
    let before = files.map(|file| fs::read(workspace.join(file)).expect("read workspace file"));

    for args in [
        vec!["missing"],
        vec!["is-positive", "missing"],
        vec!["--filter=project-2", "is-positive"],
        vec!["--save-dev", "is-positive"],
        vec!["--save-optional", "is-positive"],
        vec!["--save-prod", "is-negative"],
    ] {
        let output = pacquet_at(&workspace)
            .with_args(["remove", "-r", "--lockfile-only"])
            .with_args(&args)
            .output()
            .expect("run recursive remove");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "removal {args:?} must fail: {stderr}");
        assert!(stderr.contains("ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS"), "{args:?}: {stderr}");
        assert_eq!(
            files.map(|file| fs::read(workspace.join(file)).expect("read workspace file")),
            before,
        );
        assert!(!workspace.join("pnpm-lock.yaml").exists());
    }

    drop(root);
}

#[test]
fn recursive_remove_accepts_dependencies_spread_across_selected_projects() {
    let CommandTempCwd { pacquet, workspace, root, .. } = CommandTempCwd::init();
    prepare_removal_workspace(&workspace, ["dependencies", "devDependencies"]);

    pacquet
        .with_args(["remove", "-r", "--lockfile-only", "is-positive", "is-negative"])
        .assert()
        .success();

    assert!(!manifest_has(&workspace.join("project-1"), DependencyGroup::Prod, "is-positive"));
    assert!(!manifest_has(&workspace.join("project-2"), DependencyGroup::Dev, "is-negative"));
    assert!(workspace.join("pnpm-lock.yaml").exists());

    drop(root);
}

#[test]
fn recursive_remove_accepts_dependencies_declared_only_as_peers() {
    let CommandTempCwd { pacquet, workspace, root, .. } = CommandTempCwd::init();
    prepare_removal_workspace(&workspace, ["peerDependencies", "peerDependencies"]);

    pacquet
        .with_args(["remove", "-r", "--lockfile-only", "is-positive", "is-negative"])
        .assert()
        .success();

    assert!(!manifest_has(&workspace.join("project-1"), DependencyGroup::Peer, "is-positive"));
    assert!(!manifest_has(&workspace.join("project-2"), DependencyGroup::Peer, "is-negative"));

    drop(root);
}

#[test]
fn recursive_remove_does_nothing_when_no_projects_match_the_filter() {
    let CommandTempCwd { pacquet, workspace, root, .. } = CommandTempCwd::init();
    prepare_removal_workspace(&workspace, ["dependencies", "devDependencies"]);
    let files = ["project-1/package.json", "project-2/package.json", "pnpm-workspace.yaml"];
    let before = files.map(|file| fs::read(workspace.join(file)).expect("read workspace file"));

    pacquet
        .with_args(["remove", "-r", "--filter=absent-project", "is-positive"])
        .assert()
        .success();

    assert_eq!(
        files.map(|file| fs::read(workspace.join(file)).expect("read workspace file")),
        before,
    );
    assert!(!workspace.join("pnpm-lock.yaml").exists());

    drop(root);
}

fn prepare_removal_workspace(workspace: &Path, fields: [&str; 2]) {
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - project-*\n")
        .expect("write workspace manifest");
    for (name, group, dependency) in
        [("project-1", fields[0], "is-positive"), ("project-2", fields[1], "is-negative")]
    {
        let project_dir = workspace.join(name);
        fs::create_dir(&project_dir).expect("create project directory");
        fs::write(
            project_dir.join("package.json"),
            serde_json::json!({ "name": name, "version": "1.0.0", group: { dependency: "1.0.0" } })
                .to_string(),
        )
        .expect("write project manifest");
    }
}
