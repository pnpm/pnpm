use std::path::Path;

use super::{AddMockedRegistry, CommandExtra, CommandTempCwd, fs, new_pacquet_command};
use assert_cmd::assert::OutputAssertExt;

fn read_installed_name_and_version(workspace: &Path, alias: &str) -> (String, String) {
    let manifest_path = workspace
        .join("node_modules")
        .join(alias)
        .join("package.json");
    let raw = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", manifest_path.display()));
    let value: serde_json::Value = serde_json::from_str(&raw).expect("parse installed manifest");
    let name = value["name"]
        .as_str()
        .expect("package name")
        .to_string();
    let version = value["version"]
        .as_str()
        .expect("package version")
        .to_string();
    (name, version)
}

/// Regression test for <https://github.com/pnpm/pnpm/issues/18169>.
#[test]
fn npm_alias_with_space_separated_comparators_installs_aliased_package() {
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
            "name": "repro",
            "version": "1.0.0",
            "private": true,
            "dependencies": {
                "my-is-positive": "npm:is-positive@>=2.0.0 <3.0.0",
                "is-negative": "npm:is-positive@>=2.0.0 <3.0.0",
                "hyphen-positive": "npm:is-positive@1.0.0 - 2.5.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(
        read_installed_name_and_version(&workspace, "my-is-positive"),
        ("is-positive".to_string(), "2.0.0".to_string()),
    );
    assert_eq!(
        read_installed_name_and_version(&workspace, "is-negative"),
        ("is-positive".to_string(), "2.0.0".to_string()),
    );
    assert_eq!(
        read_installed_name_and_version(&workspace, "hyphen-positive"),
        ("is-positive".to_string(), "2.0.0".to_string()),
    );

    new_pacquet_command(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    drop((root, mock_instance));
}

/// Adding an `npm:<pkg>@<range>` alias with space-separated comparators under an
/// alias whose own package name is already in `pnpm-lock.yaml` must resolve the
/// aliased target rather than fast-updating onto the locked package of the same
/// outer name.
#[test]
fn npm_alias_with_space_separated_comparators_bypasses_same_name_fast_update() {
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
            "name": "repro",
            "version": "1.0.0",
            "private": true,
            "dependencies": {
                "keep-neg": "npm:is-negative@1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write initial package.json");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "repro",
            "version": "1.0.0",
            "private": true,
            "dependencies": {
                "keep-neg": "npm:is-negative@1.0.0",
                "is-negative": "npm:is-positive@>=2.0.0 <3.0.0",
            },
        })
        .to_string(),
    )
    .expect("write updated package.json");

    new_pacquet_command(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(
        read_installed_name_and_version(&workspace, "is-negative"),
        ("is-positive".to_string(), "2.0.0".to_string()),
    );

    drop((root, mock_instance));
}
