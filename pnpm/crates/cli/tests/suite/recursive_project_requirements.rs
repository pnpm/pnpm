//! A recursive `run` or `exec` checking what every workspace project's
//! manifest requires of the host before it runs code in them.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

/// Node.js code that creates `ran` in the project directory it runs in.
const MARKER_CODE: &str = "require('fs').writeFileSync('ran', '')";

/// The recursive commands that run code in every selected project.
const COMMANDS: [&str; 2] = ["run", "exec"];

/// A workspace of the root and `packages/app` and `packages/lib`, each
/// project with a `build` script that runs [`MARKER_CODE`]. `app` and the
/// root add `app_extra` and `root_extra` to their manifests.
fn write_workspace(workspace: &Path, app_extra: &Value, root_extra: &Value) {
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    write_manifest(workspace, "root", root_extra);
    write_manifest(&workspace.join("packages/app"), "app", app_extra);
    write_manifest(&workspace.join("packages/lib"), "lib", &json!({}));
}

fn write_manifest(dir: &Path, name: &str, extra: &Value) {
    let mut manifest = json!({
        "name": name,
        "version": "1.0.0",
        "scripts": { "build": format!(r#"node -e "{MARKER_CODE}""#) },
    });
    manifest
        .as_object_mut()
        .expect("an object")
        .extend(
            extra
                .as_object()
                .expect("an object")
                .clone(),
        );
    fs::create_dir_all(dir).expect("create the project dir");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn ran_in(workspace: &Path, project: &str) -> bool {
    workspace
        .join(project)
        .join("ran")
        .exists()
}

/// The value of `os` no host matches but this one would.
fn every_os_but_this_one() -> Value {
    json!([format!("!{}", pnpm_graph_hasher::host_platform())])
}

/// Run `pnpm -r run build`, or the `exec` that does what `build` does, in
/// the workspace.
fn run_everywhere(pacquet: Command, command: &str) -> Output {
    let args = match command {
        "run" => vec!["run", "build"],
        _ => vec!["exec", "node", "-e", MARKER_CODE],
    };
    pacquet
        .with_args(["-r", "--config.verify-deps-before-run=false"])
        .with_args(args)
        .output()
        .expect("run pnpm")
}

#[test]
fn a_project_with_an_incompatible_pnpm_version_fails() {
    for command in COMMANDS {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_workspace(&workspace, &json!({ "engines": { "pnpm": ">=99.0.0" } }), &json!({}));

        let output = run_everywhere(pacquet, command);

        let text = output_text(&output);
        assert!(!output.status.success(), "{command}: {text}");
        assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
        assert!(text.contains(r#"{"pnpm":">=99.0.0"}"#), "{text}");
        assert!(!ran_in(&workspace, "packages/app"), "{text}");
        assert!(!ran_in(&workspace, "packages/lib"), "no project runs: {text}");
        drop(root);
    }
}

/// The root project is checked although `-r` leaves it out of the run.
#[test]
fn a_root_project_with_an_incompatible_pnpm_version_fails() {
    for command in COMMANDS {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_workspace(&workspace, &json!({}), &json!({ "engines": { "pnpm": ">=99.0.0" } }));

        let output = run_everywhere(pacquet, command);

        let text = output_text(&output);
        assert!(!output.status.success(), "{command}: {text}");
        assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
        assert!(!ran_in(&workspace, "packages/app"), "{text}");
        drop(root);
    }
}

#[test]
fn a_project_node_mismatch_warns_without_engine_strict() {
    for command in COMMANDS {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_workspace(&workspace, &json!({ "engines": { "node": ">=99.0.0" } }), &json!({}));

        let output = run_everywhere(pacquet, command);

        let text = output_text(&output);
        assert!(output.status.success(), "{command}: {text}");
        assert!(text.contains("Unsupported engine for"), "{text}");
        assert!(text.contains(r#"{"node":">=99.0.0"}"#), "{text}");
        assert!(ran_in(&workspace, "packages/app"), "{text}");
        drop(root);
    }
}

#[test]
fn engine_strict_rejects_a_project_node_mismatch() {
    for command in COMMANDS {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_workspace(&workspace, &json!({ "engines": { "node": ">=99.0.0" } }), &json!({}));
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\nengineStrict: true\nnodeVersion: 20.0.0\n",
        )
        .expect("write workspace settings");

        let output = run_everywhere(pacquet, command);

        let text = output_text(&output);
        assert!(!output.status.success(), "{command}: {text}");
        assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
        assert!(text.contains(r#"{"node":"20.0.0"}"#), "{text}");
        assert!(!ran_in(&workspace, "packages/app"), "{text}");
        drop(root);
    }
}

#[test]
fn a_project_on_an_unsupported_platform_warns() {
    for command in COMMANDS {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_workspace(&workspace, &json!({ "os": every_os_but_this_one() }), &json!({}));

        let output = run_everywhere(pacquet, command);

        let text = output_text(&output);
        assert!(output.status.success(), "{command}: {text}");
        assert!(text.contains("Unsupported platform for"), "{text}");
        assert!(ran_in(&workspace, "packages/app"), "{text}");
        drop(root);
    }
}

/// A rejected run starts no run journal, so it keeps the progress an
/// interrupted run saved for `--resume-from`.
#[test]
fn a_rejected_run_starts_no_run_journal() {
    for command in COMMANDS {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_workspace(&workspace, &json!({ "engines": { "pnpm": ">=99.0.0" } }), &json!({}));
        fs::create_dir_all(workspace.join("node_modules")).expect("create node_modules");

        let output = run_everywhere(pacquet, command);

        let text = output_text(&output);
        assert!(!output.status.success(), "{command}: {text}");
        let state_dir = workspace.join("node_modules/.pnpm-task-run-state-v1");
        assert!(!state_dir.exists(), "{command}: {text}");
        drop(root);
    }
}

/// `pnpm -r <name>` hands a name no project has a script for to `exec`,
/// which checks the projects once.
#[test]
fn a_recursive_run_falling_back_to_exec_warns_once() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &json!({ "engines": { "node": ">=99.0.0" } }), &json!({}));

    let output = pacquet
        .with_args(["-r", "--config.verify-deps-before-run=false", "node", "-e"])
        .with_arg(MARKER_CODE)
        .output()
        .expect("run pnpm -r node");

    let text = output_text(&output);
    assert!(output.status.success(), "{text}");
    assert_eq!(text.matches("Unsupported engine for").count(), 1, "{text}");
    assert!(ran_in(&workspace, "packages/app"), "{text}");
    drop(root);
}
