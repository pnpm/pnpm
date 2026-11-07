//! A recursive `run` or `exec` checking what every workspace project's
//! manifest requires of the host before it runs code in them.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Output};

/// Creates `ran` in the project directory it runs in.
const MARKER_SCRIPT: &str = r#"node -e "require('fs').writeFileSync('ran', '')""#;

/// A workspace of the root and `packages/app` and `packages/lib`, each
/// project with a `build` script that runs [`MARKER_SCRIPT`]. `app` and the
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
        "scripts": { "build": MARKER_SCRIPT },
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

fn run_build(pacquet: std::process::Command) -> Output {
    pacquet
        .with_args(["-r", "--config.verify-deps-before-run=false", "run", "build"])
        .output()
        .expect("run pnpm -r run build")
}

#[test]
fn recursive_run_fails_on_a_project_with_an_incompatible_pnpm_version() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &json!({ "engines": { "pnpm": ">=99.0.0" } }), &json!({}));

    let output = run_build(pacquet);

    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
    assert!(text.contains(r#"{"pnpm":">=99.0.0"}"#), "{text}");
    assert!(!ran_in(&workspace, "packages/app"), "{text}");
    assert!(!ran_in(&workspace, "packages/lib"), "no project runs: {text}");
    drop(root);
}

#[test]
fn recursive_exec_fails_on_a_project_with_an_incompatible_pnpm_version() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &json!({ "engines": { "pnpm": ">=99.0.0" } }), &json!({}));

    let output = pacquet
        .with_args(["-r", "--config.verify-deps-before-run=false", "exec", "node", "-e"])
        .with_arg("require('fs').writeFileSync('ran', '')")
        .output()
        .expect("run pnpm -r exec");

    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
    assert!(!ran_in(&workspace, "packages/app"), "{text}");
    assert!(!ran_in(&workspace, "packages/lib"), "no project runs: {text}");
    drop(root);
}

/// The root project is checked although `run -r` leaves it out of the run.
#[test]
fn recursive_run_fails_on_a_root_project_with_an_incompatible_pnpm_version() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &json!({}), &json!({ "engines": { "pnpm": ">=99.0.0" } }));

    let output = run_build(pacquet);

    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
    assert!(!ran_in(&workspace, "packages/app"), "{text}");
    drop(root);
}

#[test]
fn recursive_run_warns_about_a_project_node_mismatch_without_engine_strict() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &json!({ "engines": { "node": ">=99.0.0" } }), &json!({}));

    let output = run_build(pacquet);

    let text = output_text(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Unsupported engine for"), "{text}");
    assert!(text.contains(r#"{"node":">=99.0.0"}"#), "{text}");
    assert!(ran_in(&workspace, "packages/app"), "{text}");
    drop(root);
}

#[test]
fn engine_strict_rejects_a_project_node_mismatch_on_a_recursive_run() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &json!({ "engines": { "node": ">=99.0.0" } }), &json!({}));
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nengineStrict: true\nnodeVersion: 20.0.0\n",
    )
    .expect("write workspace settings");

    let output = run_build(pacquet);

    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "{text}");
    assert!(text.contains(r#"{"node":"20.0.0"}"#), "{text}");
    assert!(!ran_in(&workspace, "packages/app"), "{text}");
    drop(root);
}

#[test]
fn recursive_run_warns_about_a_project_on_an_unsupported_platform() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &json!({ "os": every_os_but_this_one() }), &json!({}));

    let output = run_build(pacquet);

    let text = output_text(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Unsupported platform for"), "{text}");
    assert!(ran_in(&workspace, "packages/app"), "{text}");
    drop(root);
}

/// `pnpm -r <name>` hands a name no project has a script for to `exec`,
/// which checks the projects once.
#[test]
fn a_recursive_run_falling_back_to_exec_warns_once() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &json!({ "engines": { "node": ">=99.0.0" } }), &json!({}));

    let output = pacquet
        .with_args(["-r", "--config.verify-deps-before-run=false", "node", "-e"])
        .with_arg("require('fs').writeFileSync('ran', '')")
        .output()
        .expect("run pnpm -r node");

    let text = output_text(&output);
    assert!(output.status.success(), "{text}");
    assert_eq!(text.matches("Unsupported engine for").count(), 1, "{text}");
    assert!(ran_in(&workspace, "packages/app"), "{text}");
    drop(root);
}
