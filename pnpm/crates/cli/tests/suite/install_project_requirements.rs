//! An install checking what every workspace project's manifest requires of
//! the host: its `engines`, `os`, `cpu`, and `libc`.

use crate::{
    _utils,
    repeat_install_records::{DECIDED_FROM_RECORDS, traced_install, trust_recorded_stats},
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Output};

/// A workspace of the root and `packages/app`, whose manifest adds `extra`.
fn write_workspace(workspace: &Path, extra: &serde_json::Value) {
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).unwrap_or_default();
    yaml.push_str("packages:\n  - packages/*\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write the root package.json");
    let mut manifest = serde_json::json!({ "name": "app", "version": "1.0.0" });
    manifest
        .as_object_mut()
        .expect("an object")
        .extend(
            extra
                .as_object()
                .expect("an object")
                .clone(),
        );
    fs::create_dir_all(workspace.join("packages/app")).expect("create packages/app");
    fs::write(workspace.join("packages/app/package.json"), manifest.to_string())
        .expect("write packages/app/package.json");
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

/// The value of `os` no host matches but this one would.
fn every_os_but_this_one() -> serde_json::Value {
    serde_json::json!([format!("!{}", pnpm_graph_hasher::host_platform())])
}

#[test]
fn a_workspace_project_with_an_incompatible_pnpm_version_fails() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &serde_json::json!({ "engines": { "pnpm": ">=99.0.0" } }));

    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install");

    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
    assert!(text.contains(r#"{"pnpm":">=99.0.0"}"#), "{text}");
    assert!(text.contains("app"), "the error names the project: {text}");
    assert!(!workspace.join("pnpm-lock.yaml").exists(), "{text}");
    drop(root);
}

#[test]
fn a_workspace_project_node_mismatch_warns_without_engine_strict() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &serde_json::json!({ "engines": { "node": ">=99.0.0" } }));

    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install");

    let text = output_text(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Unsupported engine for"), "{text}");
    assert!(text.contains(r#"{"node":">=99.0.0"}"#), "{text}");
    drop(root);
}

#[test]
fn engine_strict_rejects_a_workspace_project_node_mismatch() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "engineStrict: true\nnodeVersion: 20.0.0\n")
        .expect("write workspace settings");
    write_workspace(&workspace, &serde_json::json!({ "engines": { "node": ">=99.0.0" } }));

    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install");

    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
    assert!(text.contains(r#"{"node":"20.0.0"}"#), "{text}");
    drop(root);
}

#[test]
fn a_workspace_project_checks_node_engines_against_its_runtime() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "engineStrict: true\nruntimeOnFail: download\n",
    )
    .expect("write workspace settings");
    write_workspace(
        &workspace,
        &serde_json::json!({
            "engines": { "node": ">=99.0.0" },
            "devEngines": { "runtime": { "name": "node", "version": "^99.0.0" } },
        }),
    );

    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install");

    assert!(output.status.success(), "{}", output_text(&output));
    drop(root);
}

#[test]
fn a_workspace_project_on_an_unsupported_platform_warns() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "engineStrict: true\n")
        .expect("write workspace settings");
    write_workspace(&workspace, &serde_json::json!({ "os": every_os_but_this_one() }));

    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install");

    let text = output_text(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Unsupported platform for"), "{text}");
    drop(root);
}

#[test]
fn the_root_project_on_an_unsupported_platform_warns() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "os": every_os_but_this_one() }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install");

    let text = output_text(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Unsupported platform for"), "{text}");
    drop(root);
}

#[test]
fn engine_strict_on_a_repeat_install_rejects_a_workspace_project() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &serde_json::json!({ "engines": { "node": ">=99.0.0" } }));
    _utils::pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let output = _utils::pacquet_in(&workspace)
        .with_args(["install", "--engine-strict"])
        .output()
        .expect("run the repeat install");

    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
    drop(root);
}

#[test]
fn a_repeat_install_decided_from_the_records_warns_about_a_workspace_project() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, &serde_json::json!({ "engines": { "node": ">=99.0.0" } }));
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());

    let output = traced_install(&workspace, mock_instance.url());

    assert!(output.contains(DECIDED_FROM_RECORDS), "{output}");
    assert!(output.contains("Already up to date"), "{output}");
    assert!(output.contains("Unsupported engine for"), "{output}");
    drop((root, mock_instance));
}

/// A record written before the records kept `engines` would stand in for a
/// manifest without them, so it is not used until a check rewrites it.
#[test]
fn a_record_of_an_older_field_revision_is_read_again() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, &serde_json::json!({ "engines": { "node": ">=99.0.0" } }));
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    trust_recorded_stats(&workspace, mock_instance.url());
    downgrade_manifest_records(&workspace);

    let from_files = traced_install(&workspace, mock_instance.url());
    let from_records = traced_install(&workspace, mock_instance.url());

    assert!(!from_files.contains(DECIDED_FROM_RECORDS), "{from_files}");
    assert!(from_files.contains("Unsupported engine for"), "{from_files}");
    assert!(from_records.contains(DECIDED_FROM_RECORDS), "{from_records}");
    assert!(from_records.contains("Unsupported engine for"), "{from_records}");
    drop((root, mock_instance));
}

/// Make every manifest record look like one 12.18 wrote: no revision and
/// none of the fields that were added with one.
fn downgrade_manifest_records(workspace: &Path) {
    let state_path = workspace.join("node_modules/.pnpm-workspace-state-v1.json");
    let mut state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read the state"))
            .expect("parse the state");
    let records = state["validation"]["manifestFiles"].as_object_mut().expect("manifest records");
    for record in records.values_mut() {
        let record = record.as_object_mut().expect("a record");
        record.remove("fieldsRevision").expect("a fields revision");
        let fields = record["fields"].as_object_mut().expect("recorded fields");
        for field in ["engines", "os", "cpu", "libc"] {
            fields.remove(field);
        }
    }
    fs::write(&state_path, state.to_string()).expect("write the state");
}

/// A project on an unsupported platform is still held to its `engines`.
#[test]
fn a_platform_mismatch_does_not_hide_an_engine_failure() {
    for in_workspace in [false, true] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        let requirements = serde_json::json!({
            "os": every_os_but_this_one(),
            "engines": { "pnpm": ">=99.0.0" },
        });
        if in_workspace {
            write_workspace(&workspace, &requirements);
        } else {
            let mut manifest = requirements.clone();
            manifest["name"] = "root".into();
            fs::write(workspace.join("package.json"), manifest.to_string())
                .expect("write package.json");
        }

        let output = pacquet
            .with_args(["install", "--lockfile-only"])
            .output()
            .expect("run pnpm install");

        let text = output_text(&output);
        assert!(!output.status.success(), "in a workspace: {in_workspace}: {text}");
        assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
        drop(root);
    }
}

#[test]
fn engine_strict_on_a_repeat_install_rejects_a_project_on_an_unsupported_platform() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &serde_json::json!({
            "os": every_os_but_this_one(),
            "engines": { "node": ">=99.0.0" },
        }),
    );
    _utils::pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let output = _utils::pacquet_in(&workspace)
        .with_args(["install", "--engine-strict"])
        .output()
        .expect("run the repeat install");

    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
    drop(root);
}

#[test]
fn adding_to_one_project_checks_the_other_workspace_projects() {
    for shared_lockfile in [true, false] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            format!("sharedWorkspaceLockfile: {shared_lockfile}\n"),
        )
        .expect("write workspace settings");
        write_workspace(&workspace, &serde_json::json!({ "engines": { "pnpm": ">=99.0.0" } }));

        let output = pacquet
            .with_args(["add", "--workspace-root", "app@workspace:*"])
            .output()
            .expect("run pnpm add");

        let text = output_text(&output);
        assert!(!output.status.success(), "shared lockfile: {shared_lockfile}: {text}");
        assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
        let manifest =
            fs::read_to_string(workspace.join("package.json")).expect("read package.json");
        assert!(!manifest.contains("dependencies"), "{manifest}");
        drop(root);
    }
}
