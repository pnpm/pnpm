use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{
        AddMockedRegistry,
        CommandTempCwd,
    },
    command_env::CommandTestExt,
};
use std::fs;

#[test]
fn link_fails_without_paths() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_arg("link")
        .output()
        .expect("spawn pacquet link");
    assert!(!output.status.success(), "link without paths must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("You must provide a parameter"),
        "stderr should contain error message: {stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn ln_alias_fails_without_paths() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_arg("ln")
        .output()
        .expect("spawn pacquet ln");
    assert!(!output.status.success(), "ln alias without paths must fail");

    drop((root, mock_instance));
}

#[test]
fn link_fails_with_nonexistent_target() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_arg("link")
        .with_arg(
            workspace
                .join("definitely-missing-target")
                .to_string_lossy()
                .as_ref(),
        )
        .output()
        .expect("spawn pacquet link");
    assert!(!output.status.success(), "link to nonexistent path must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No package.json found"),
        "stderr should contain error message: {stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn link_succeeds_with_valid_target() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("target-project");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "target-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet
        .with_arg("link")
        .with_arg("../target-project")
        .assert()
        .success();

    let manifest =
        pnpm_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    let deps = manifest.value()["dependencies"].as_object().expect("dependencies exist");
    assert!(deps.contains_key("target-project"), "dependency must exist");
    assert_eq!(deps["target-project"], "link:../target-project");

    drop((root, mock_instance));
}

#[test]
fn link_succeeds_with_absolute_path() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("abs-target");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "abs-target", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet
        .with_arg("link")
        .with_arg(target_dir.to_string_lossy().as_ref())
        .assert()
        .success();

    let manifest =
        pnpm_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    let deps = manifest.value()["dependencies"].as_object().expect("dependencies exist");
    assert!(deps.contains_key("abs-target"), "dependency must exist");

    drop((root, mock_instance));
}

#[test]
fn link_succeeds_with_multiple_targets() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_a = root.path().join("multi-a");
    fs::create_dir_all(&target_a).expect("create target dir");
    fs::write(
        target_a.join("package.json"),
        serde_json::json!({ "name": "multi-a", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    let target_b = root.path().join("multi-b");
    fs::create_dir_all(&target_b).expect("create target dir");
    fs::write(
        target_b.join("package.json"),
        serde_json::json!({ "name": "multi-b", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet
        .with_arg("link")
        .with_arg("../multi-a")
        .with_arg("../multi-b")
        .assert()
        .success();

    let manifest =
        pnpm_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    let deps = manifest.value()["dependencies"].as_object().expect("dependencies exist");
    assert!(deps.contains_key("multi-a"), "dependency multi-a must exist");
    assert!(deps.contains_key("multi-b"), "dependency multi-b must exist");

    drop((root, mock_instance));
}

#[test]
fn link_expands_home_and_uses_directory_name_for_unnamed_package() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("target-project-no-name");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    let home = dunce::canonicalize(root.path()).expect("resolve the home directory");
    pacquet
        .with_env("HOME", &home)
        .with_env("USERPROFILE", &home)
        .with_args(["link", "~/target-project-no-name/child/.."])
        .assert()
        .success();
    let manifest =
        pnpm_package_manifest::PackageManifest::from_path(workspace.join("package.json")).unwrap();
    assert_eq!(
        manifest.value()["dependencies"]["target-project-no-name"],
        "link:../target-project-no-name",
    );

    drop((root, mock_instance));
}

#[test]
fn link_refuses_invalid_target_name_without_writing() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest = serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string();
    fs::write(workspace.join("package.json"), &manifest).expect("write package.json");
    let read_workspace_yaml = || {
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read pnpm-workspace.yaml")
    };
    let workspace_yaml = read_workspace_yaml();

    let target_dir = root.path().join("invalid-name");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "mcp-use server", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    let output = pacquet
        .with_arg("link")
        .with_arg("../invalid-name")
        .output()
        .expect("spawn");
    assert!(!output.status.success(), "link to a target with an invalid name must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_INVALID_DEPENDENCY_NAME"), "stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
        manifest,
    );
    assert_eq!(read_workspace_yaml(), workspace_yaml);

    drop((root, mock_instance));
}

/// A relative path is resolved against the directory the command runs in,
/// not against the nearest `package.json` above it.
#[test]
fn link_resolves_relative_path_from_current_directory() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");
    let tools_dir = workspace.join("tools");
    fs::create_dir_all(&tools_dir).unwrap();
    let target_dir = workspace.join("tools-lib");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet
        .with_current_dir(&tools_dir)
        .with_arg("link")
        .with_arg("../tools-lib")
        .assert()
        .success();

    let manifest =
        pnpm_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    let deps = manifest.value()["dependencies"].as_object().expect("dependencies exist");
    assert_eq!(deps["tools-lib"], "link:tools-lib");
    assert!(!tools_dir.join("package.json").exists());

    drop((root, mock_instance));
}

/// `pnpm link .` in a directory without a `package.json` must not link the
/// project above it into itself.
#[test]
fn link_dot_in_directory_without_manifest_does_not_link_parent_project() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest = serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string();
    fs::write(workspace.join("package.json"), &manifest).expect("write package.json");
    let read_workspace_yaml = || {
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read pnpm-workspace.yaml")
    };
    let workspace_yaml = read_workspace_yaml();
    let packages_dir = workspace.join("packages");
    fs::create_dir_all(&packages_dir).expect("create packages dir");

    let output = pacquet
        .with_current_dir(&packages_dir)
        .with_arg("link")
        .with_arg(".")
        .output()
        .expect("spawn pacquet link");
    assert!(!output.status.success(), "linking a directory without package.json must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No package.json found"), "stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
        manifest,
    );
    assert_eq!(read_workspace_yaml(), workspace_yaml);

    drop((root, mock_instance));
}

#[test]
fn ln_alias_succeeds_with_valid_target() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("ln-target");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "ln-target", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet
        .with_arg("ln")
        .with_arg("../ln-target")
        .assert()
        .success();

    let manifest =
        pnpm_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    let deps = manifest.value()["dependencies"].as_object().expect("dependencies exist");
    assert!(deps.contains_key("ln-target"), "dependency must exist");
    assert_eq!(deps["ln-target"], "link:../ln-target");

    drop((root, mock_instance));
}

/// `link` records the `link:` specifier in `pnpm-workspace.yaml`'s
/// `overrides:` block (mirroring pnpm), not just in `dependencies`.
#[test]
fn link_persists_override_to_workspace_yaml() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("override-target");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "override-target", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet
        .with_arg("link")
        .with_arg("../override-target")
        .assert()
        .success();

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    assert!(workspace_yaml.contains("overrides:"), "overrides block must exist: {workspace_yaml}");
    assert!(
        workspace_yaml.contains("override-target: link:../override-target"),
        "override must record the link spec: {workspace_yaml}",
    );

    drop((root, mock_instance));
}

/// When the package is already declared in another dependency field, `link`
/// records the override but does not add a duplicate `dependencies` entry,
/// matching pnpm's `DEPENDENCIES_FIELDS.every(...)` guard. The override is
/// what makes the `link:` spec win, so the install links the local target
/// even though the existing spec is a registry range.
#[test]
fn link_existing_dependency_writes_override_only() {
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
        serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "devDependencies": { "target-project": "^1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("target-project");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "target-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet
        .with_arg("link")
        .with_arg("../target-project")
        .assert()
        .success();

    let manifest =
        pnpm_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    assert!(
        manifest
            .value()
            .get("dependencies")
            .is_none(),
        "no dependencies entry should be added when already declared elsewhere",
    );
    let dev_deps = manifest.value()["devDependencies"].as_object().expect("devDependencies exist");
    assert_eq!(dev_deps["target-project"], "^1.0.0", "the existing entry stays untouched");

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    assert!(
        workspace_yaml.contains("target-project: link:../target-project"),
        "override must record the link spec: {workspace_yaml}",
    );

    // The install succeeded despite the registry-spec devDependency, which is
    // only possible because the in-memory override rewrote it to the local
    // `link:`, confirm the symlink was created rather than a registry fetch.
    let linked = workspace.join("node_modules").join("target-project");
    assert!(
        fs::symlink_metadata(&linked)
            .map(|meta| meta.file_type())
            .is_ok_and(|file_type| file_type.is_symlink()),
        "node_modules/target-project must be a symlink created from the link override",
    );

    drop((root, mock_instance));
}

#[test]
fn link_warns_about_peer_dependencies() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("linked-with-peer-deps");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({
            "name": "linked-with-peer-deps",
            "version": "1.0.0",
            "peerDependencies": {
                "some-peer-dependency": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write target package.json");

    let output = pacquet
        .with_args(["--reporter=ndjson", "link", "../linked-with-peer-deps"])
        .output()
        .expect("spawn pacquet link");
    assert!(output.status.success(), "link should succeed: {output:?}");

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    let warn_events: Vec<serde_json::Value> = stderr
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .filter_map(Result::ok)
        .filter(|val| {
            val.get("name").and_then(serde_json::Value::as_str) == Some("pnpm")
                && val.get("level").and_then(serde_json::Value::as_str) == Some("warn")
        })
        .collect();

    assert_eq!(
        warn_events.len(),
        1,
        "expected exactly 1 warn event on pnpm channel, got:\n{stderr}",
    );
    let warn_event = &warn_events[0];
    let message = warn_event
        .get("message")
        .and_then(serde_json::Value::as_str)
        .expect("message string");
    assert!(
        message.contains("has the following peerDependencies specified in its package.json"),
        "message must warn about peerDependencies:\n{message}",
    );
    assert!(
        message.contains("The linked in dependency will not resolve the peer dependencies from the target node_modules."),
        "message must explain the limitation:\n{message}",
    );
    assert!(
        message.contains(r#"To resolve this, you may use the "file:" protocol to reference the local dependency."#),
        "message must suggest file: protocol:\n{message}",
    );

    drop((root, mock_instance));
}

#[test]
fn link_does_not_warn_when_peer_dependencies_empty() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("linked-with-empty-peer-deps");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({
            "name": "linked-with-empty-peer-deps",
            "version": "1.0.0",
            "peerDependencies": {}
        })
        .to_string(),
    )
    .expect("write target package.json");

    let output = pacquet
        .with_arg("link")
        .with_arg("../linked-with-empty-peer-deps")
        .output()
        .expect("spawn pacquet link");
    assert!(output.status.success(), "link should succeed: {output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        !combined.contains("has the following peerDependencies specified in its package.json"),
        "output must not warn about empty peerDependencies:\n{combined}",
    );

    drop((root, mock_instance));
}

#[tokio::test]
async fn link_sanitizes_control_characters_in_peer_deps_warning() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("linked-with-control-chars");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({
            "name": "malicious-pkg",
            "version": "1.0.0",
            "peerDependencies": {
                "react\x1b[0m\r\n": "^18.0.0\x1b[32m\r"
            }
        })
        .to_string(),
    )
    .expect("write target package.json");

    let output = pacquet
        .with_arg("link")
        .with_arg("../linked-with-control-chars")
        .output()
        .expect("spawn pacquet link");
    assert!(output.status.success(), "link should succeed: {output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    assert!(
        combined.contains("The package malicious-pkg, which you have just pnpm linked"),
        "sanitized package name must be in output:\n{combined}",
    );
    assert!(
        combined.contains("react@^18.0.0"),
        "sanitized peer dependency must be in output:\n{combined}",
    );
    assert!(
        !combined.contains('\x1b'),
        "output must not contain ANSI escape sequence:\n{combined}",
    );
    assert!(!combined.contains('\r'), "output must not contain carriage return:\n{combined}");

    drop((root, mock_instance));
}

/// `--hard` saves the dependency and its override with the `file:` protocol,
/// so the package is installed as a directory of hard-linked files instead of
/// a symlink to the source.
#[test]
fn link_hard_uses_file_protocol() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("hard-target");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "hard-target", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet
        .with_args(["link", "--hard", "../hard-target"])
        .assert()
        .success();

    let manifest =
        pnpm_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    assert_eq!(manifest.value()["dependencies"]["hard-target"], "file:../hard-target");

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    assert!(
        workspace_yaml.contains("hard-target: file:../hard-target"),
        "override must record the file: spec: {workspace_yaml}",
    );

    let installed = workspace.join("node_modules/hard-target");
    let metadata = fs::symlink_metadata(&installed).expect("hard-target is installed");
    let resolved = fs::canonicalize(&installed).expect("resolve installed hard-target");
    assert!(
        resolved != fs::canonicalize(&target_dir).expect("resolve target dir"),
        "hard-target must not resolve to the source directory: {metadata:?}",
    );
    assert!(resolved.join("package.json").is_file(), "the package files must be installed");

    drop((root, mock_instance));
}

/// A hard-linked package resolves its peers from the project, so the warning
/// about unresolved peers of a symlinked package does not apply.
#[test]
fn link_hard_does_not_warn_about_peer_dependencies() {
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
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("hard-with-peer-deps");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({
            "name": "hard-with-peer-deps",
            "version": "1.0.0",
            "peerDependencies": {
                "@pnpm.e2e/foo": "100.0.0"
            }
        })
        .to_string(),
    )
    .expect("write target package.json");

    let output = pacquet
        .with_args(["--reporter=ndjson", "link", "--hard", "../hard-with-peer-deps"])
        .output()
        .expect("spawn pacquet link");
    assert!(output.status.success(), "link should succeed: {output:?}");

    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert!(
        !stderr.contains("which you have just pnpm linked"),
        "--hard must not warn about peers of a symlinked package:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// A `file:` specifier ending in `.tgz`, `.tar.gz`, or `.tar` names a
/// tarball, so `--hard` refuses a directory with such a name before it
/// writes anything.
#[test]
fn link_hard_rejects_directory_with_tarball_name() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest = serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string();
    fs::write(workspace.join("package.json"), &manifest).expect("write package.json");
    let workspace_yaml_before =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap_or_default();

    let target_dir = root.path().join("archive-like.tgz");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "archive-like", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    // A trailing separator must not get past the check: the saved spec
    // drops it.
    for target in ["../archive-like.tgz", "../archive-like.tgz/"] {
        let output = std::process::Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config()
            .with_args(["link", "--hard", target])
            .output()
            .expect("spawn pacquet link");
        assert!(!output.status.success(), "link --hard {target} must fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("ERR_PNPM_LINK_HARD_TARBALL_NAME"),
            "stderr should name the error for {target}: {stderr}",
        );
    }

    assert_eq!(
        fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
        manifest,
        "package.json must be unchanged",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap_or_default(),
        workspace_yaml_before,
        "pnpm-workspace.yaml must be unchanged",
    );

    drop((root, mock_instance));
}
