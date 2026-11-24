use super::{
    CommandTempCwd,
    Path,
    assert_failure,
    assert_success,
    fs,
    stdout,
};
use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use std::process::{
    Command,
    Output,
};

const LEGACY_NODE: &str = "@pnpm.e2e/for-legacy-node";
const NO_OS: &str = "@pnpm.e2e/not-compatible-with-any-os";

/// A project with `manifest` as its `package.json`, a local `file:` package
/// `local` that depends on the Node.js 0.10 fixture, and a `nodeVersion`
/// that fixture does not support.
fn write_project(workspace: &Path, manifest: &serde_json::Value) {
    let local = workspace.join("local");
    fs::create_dir_all(&local).expect("create the local package");
    fs::write(
        local.join("package.json"),
        serde_json::json!({
            "name": "local",
            "version": "1.0.0",
            "dependencies": { LEGACY_NODE: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write the local package.json");
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    let settings = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&settings).unwrap_or_default();
    fs::write(&settings, format!("{yaml}\nnodeVersion: 20.0.0\n"))
        .expect("write pnpm-workspace.yaml");
}

fn pnpm(workspace: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("pnpm")
        .expect("find pnpm")
        .with_current_dir(workspace)
        .with_args(args)
        .output()
        .expect("run pnpm")
}

fn install(workspace: &Path) {
    assert_success(&pnpm(workspace, &["install", "--lockfile-only"]));
}

fn json_report(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("the report is JSON")
}

#[test]
fn lists_direct_dependencies_that_do_not_support_the_host_and_fails() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    write_project(
        &fixture.workspace,
        &serde_json::json!({ "dependencies": { LEGACY_NODE: "1.0.0", NO_OS: "1.0.0" } }),
    );
    install(&fixture.workspace);

    let output = pnpm(&fixture.workspace, &["audit", "engines"]);

    assert_failure(&output);
    let out = stdout(&output);
    assert!(
        out.contains(&format!("{LEGACY_NODE} 1.0.0  wants Node.js 0.10, not Node.js 20.0.0  .")),
        "{out}",
    );
    assert!(
        out.contains(&format!("{NO_OS} 1.0.0  wants os this-os-does-not-exist, not ")),
        "{out}",
    );
    assert!(out.contains("2 incompatible packages"), "{out}");
}

#[test]
fn names_the_chain_of_a_subdependency() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    write_project(
        &fixture.workspace,
        &serde_json::json!({ "dependencies": { "local": "file:./local" } }),
    );
    install(&fixture.workspace);

    let output = pnpm(&fixture.workspace, &["audit", "engines", "--json"]);

    assert_failure(&output);
    assert_eq!(
        json_report(&output),
        serde_json::json!([{
            "name": LEGACY_NODE,
            "version": "1.0.0",
            "unsupported": "engine",
            "wanted": { "node": "0.10" },
            "current": { "node": "20.0.0" },
            "via": ["local"],
        }]),
    );
}

/// An install skips an optional dependency that does not support the
/// host, so it is not installed.
#[test]
fn leaves_out_a_skipped_optional_dependency() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    write_project(
        &fixture.workspace,
        &serde_json::json!({ "optionalDependencies": { NO_OS: "1.0.0" } }),
    );
    install(&fixture.workspace);

    let output = pnpm(&fixture.workspace, &["audit", "engines"]);

    assert_success(&output);
    assert!(stdout(&output).contains("No incompatible packages found"));
}

#[test]
fn prod_leaves_out_a_dev_dependency() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    write_project(
        &fixture.workspace,
        &serde_json::json!({ "devDependencies": { LEGACY_NODE: "1.0.0" } }),
    );
    install(&fixture.workspace);

    assert_failure(&pnpm(&fixture.workspace, &["audit", "engines"]));
    assert_success(&pnpm(&fixture.workspace, &["audit", "engines", "--prod"]));
}
