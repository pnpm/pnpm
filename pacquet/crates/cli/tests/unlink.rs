use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_package_manifest::PackageManifest;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

#[test]
fn unlink_removes_single_link_override() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "pnpm": {
                "overrides": {
                    "foo": "link:../foo",
                    "bar": "link:../bar",
                    "baz": "1.0.0",
                }
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["unlink", "foo"]).assert().success();

    let manifest = PackageManifest::from_path(manifest_path).expect("read manifest");
    let overrides = manifest.value()["pnpm"]["overrides"].as_object();
    assert!(overrides.is_some(), "overrides must still exist");
    let overrides = overrides.unwrap();
    assert!(!overrides.contains_key("foo"), "foo must be removed from overrides");
    assert!(overrides.contains_key("bar"), "bar must remain in overrides");
    assert!(overrides.contains_key("baz"), "non-link override must remain");

    drop((root, mock_instance));
}

#[test]
fn unlink_without_args_removes_all_link_overrides() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "pnpm": {
                "overrides": {
                    "foo": "link:../foo",
                    "bar": "link:../bar",
                    "baz": "1.0.0",
                }
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("unlink").assert().success();

    let manifest = PackageManifest::from_path(manifest_path).expect("read manifest");
    let overrides = manifest.value()["pnpm"]["overrides"].as_object();
    assert!(overrides.is_some(), "overrides must still exist");
    let overrides = overrides.unwrap();
    assert!(!overrides.contains_key("foo"), "foo must be removed from overrides");
    assert!(!overrides.contains_key("bar"), "bar must be removed from overrides");
    assert!(overrides.contains_key("baz"), "non-link override must remain");

    drop((root, mock_instance));
}

#[test]
fn unlink_does_nothing_if_no_overrides() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("unlink").assert().success();

    let manifest = PackageManifest::from_path(manifest_path).expect("read manifest");
    assert!(manifest.value().get("pnpm").is_none(), "no overrides should be created");

    drop((root, mock_instance));
}

#[test]
fn unlink_ignores_non_linked_packages() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "pnpm": {
                "overrides": {
                    "baz": "1.0.0",
                }
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["unlink", "baz"]).assert().success();

    let manifest = PackageManifest::from_path(manifest_path).expect("read manifest");
    let overrides = manifest.value()["pnpm"]["overrides"].as_object().unwrap();
    assert!(overrides.contains_key("baz"), "non-link override must remain");

    drop((root, mock_instance));
}

#[test]
fn unlink_filters_non_link_packages_from_args() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "pnpm": {
                "overrides": {
                    "foo": "link:../foo",
                    "bar": "link:../bar",
                    "baz": "1.0.0",
                }
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["unlink", "foo", "baz"]).assert().success();

    let manifest = PackageManifest::from_path(manifest_path).expect("read manifest");
    let overrides = manifest.value()["pnpm"]["overrides"].as_object().unwrap();
    assert!(!overrides.contains_key("foo"), "foo (link:) must be removed");
    assert!(overrides.contains_key("baz"), "baz (non-link) must remain");
    assert!(overrides.contains_key("bar"), "bar must remain");

    drop((root, mock_instance));
}

#[test]
fn dislink_alias_works() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "pnpm": {
                "overrides": {
                    "foo": "link:../foo",
                }
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["dislink", "foo"]).assert().success();

    let manifest = PackageManifest::from_path(manifest_path).expect("read manifest");
    let overrides = manifest.value()["pnpm"]["overrides"].as_object().unwrap();
    assert!(!overrides.contains_key("foo"), "foo must be removed via dislink alias");

    drop((root, mock_instance));
}

#[test]
fn unlink_multiple_link_overrides() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "pnpm": {
                "overrides": {
                    "foo": "link:../foo",
                    "bar": "link:../bar",
                }
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["unlink", "foo", "bar"]).assert().success();

    let manifest = PackageManifest::from_path(manifest_path).expect("read manifest");
    let overrides = manifest.value()["pnpm"]["overrides"].as_object().unwrap();
    assert!(overrides.is_empty(), "all link overrides must be removed");

    drop((root, mock_instance));
}
