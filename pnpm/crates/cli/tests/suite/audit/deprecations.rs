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

const DEPRECATED: &str = "@pnpm.e2e/deprecated";

/// A project that depends on `dependencies`, and a local `file:` package
/// `local` that depends on the deprecated fixture, so a subdependency has a
/// chain to report.
fn write_project(workspace: &Path, dependencies: &serde_json::Value) {
    let local = workspace.join("local");
    fs::create_dir_all(&local).expect("create the local package");
    fs::write(
        local.join("package.json"),
        serde_json::json!({
            "name": "local",
            "version": "1.0.0",
            "dependencies": { DEPRECATED: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write the local package.json");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "project", "dependencies": dependencies }).to_string(),
    )
    .expect("write package.json");
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

#[test]
fn lists_a_deprecated_direct_dependency_and_fails() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    write_project(&fixture.workspace, &serde_json::json!({ DEPRECATED: "1.0.0" }));
    install(&fixture.workspace);

    let output = pnpm(&fixture.workspace, &["audit", "deprecations"]);

    assert_failure(&output);
    let out = stdout(&output);
    assert!(out.contains(&format!("{DEPRECATED} 1.0.0  .")), "{out}");
    assert!(out.contains("1 deprecated package"), "{out}");
}

#[test]
fn names_the_chain_of_a_deprecated_subdependency() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    write_project(&fixture.workspace, &serde_json::json!({ "local": "file:./local" }));
    install(&fixture.workspace);

    let output = pnpm(&fixture.workspace, &["audit", "deprecations", "--json"]);

    assert_failure(&output);
    let report: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("the report is JSON");
    assert_eq!(report[0]["name"], DEPRECATED);
    assert_eq!(report[0]["version"], "1.0.0");
    assert_eq!(report[0]["via"], serde_json::json!(["local"]));
    assert!(
        report[0]["deprecated"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
    );
}

#[test]
fn reports_nothing_without_deprecated_packages() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    write_project(&fixture.workspace, &serde_json::json!({ "is-positive": "1.0.0" }));
    install(&fixture.workspace);

    let output = pnpm(&fixture.workspace, &["audit", "deprecations"]);

    assert_success(&output);
    assert!(stdout(&output).contains("No deprecated packages found"));
}

#[test]
fn prod_leaves_out_a_deprecated_dev_dependency() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    write_project(&fixture.workspace, &serde_json::json!({}));
    fs::write(
        fixture.workspace.join("package.json"),
        serde_json::json!({ "name": "project", "devDependencies": { DEPRECATED: "1.0.0" } })
            .to_string(),
    )
    .expect("write package.json");
    install(&fixture.workspace);

    assert_failure(&pnpm(&fixture.workspace, &["audit", "deprecations"]));
    assert_success(&pnpm(&fixture.workspace, &["audit", "deprecations", "--prod"]));
}

/// The install cached the metadata of every package, so the audit reads
/// it from the cache.
#[test]
fn reads_the_metadata_an_install_cached() {
    let fixture = CommandTempCwd::init().add_recording_mocked_registry();
    write_project(&fixture.workspace, &serde_json::json!({ "local": "file:./local" }));
    install(&fixture.workspace);
    let _ = fixture.npmrc_info.mock_instance.take_requests();

    assert_failure(&pnpm(&fixture.workspace, &["audit", "deprecations"]));

    let requests = fixture.npmrc_info.mock_instance.take_requests();
    assert!(requests.is_empty(), "the audit asked the registry again: {requests:#?}");
}
