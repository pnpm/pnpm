//! `pacquet doctor` runs a lifecycle script whose `.bin` shim has to find
//! `node`, both in a temporary project and in the project it is run from.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn doctor(pacquet: Command, root: &Path) -> Value {
    let output = pacquet
        .with_arg(format!("--config.store-dir={}", root.join("store").display()))
        .with_arg(format!("--config.cache-dir={}", root.join("cache").display()))
        .with_args(["doctor", "--offline", "--json"])
        .output()
        .expect("run pacquet doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_str(&stdout).expect("parse the doctor report")
}

fn check<'a>(report: &'a Value, title: &str) -> &'a Value {
    report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["title"] == title)
        .unwrap_or_else(|| panic!("no {title:?} check in {report:#}"))
}

#[test]
fn runs_a_package_executable_from_a_script_in_the_current_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"doctor-project","version":"1.0.0"}"#)
        .expect("write package.json");

    let report = doctor(pacquet, root.path());

    let scripts = check(&report, "Lifecycle scripts");
    dbg!(scripts);
    assert_eq!(scripts["status"], "pass");
    let detail = scripts["detail"].as_str().expect("detail");
    assert!(detail.starts_with("an install script ran "), "{detail}");
    assert!(detail.contains(&format!("a script in {} ran ", workspace.display())), "{detail}");
    assert!(
        !workspace.join("node_modules").exists(),
        "the probe must not install into the project",
    );
}

/// A `node` that `pnpm` sees but that cannot run a script is what a script
/// fails on with exit status 127; the report has to carry the shim's trace.
#[cfg(unix)]
#[test]
fn a_failing_script_is_reported_with_the_shim_trace() {
    use std::os::unix::fs::PermissionsExt;

    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let fake_bin = root.path().join("fake-bin");
    fs::create_dir_all(&fake_bin).expect("create fake bin dir");
    let fake_node = fake_bin.join("node");
    fs::write(&fake_node, "#!/bin/sh\nexit 127\n").expect("write fake node");
    fs::set_permissions(&fake_node, fs::Permissions::from_mode(0o755)).expect("chmod fake node");
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    let report = doctor(pacquet.with_env("PATH", path), root.path());

    let node = check(&report, "Node.js on PATH");
    dbg!(node);
    assert_eq!(node["detail"], fake_node.display().to_string());
    let scripts = check(&report, "Lifecycle scripts");
    dbg!(scripts);
    assert_eq!(scripts["status"], "fail");
    let detail = scripts["detail"].as_str().expect("detail");
    assert!(detail.starts_with("the install script failed: "), "{detail}");
    assert!(detail.contains("exited with exit status: 127"), "{detail}");
    assert!(detail.contains("+ exec node "), "the trace must show the shim's exec: {detail}");
}
