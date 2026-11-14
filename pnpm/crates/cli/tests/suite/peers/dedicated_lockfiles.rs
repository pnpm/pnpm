use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn pnpm(dir: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(dir)
}

fn run_peers_json(dir: &Path, args: &[&str], expected_code: i32) -> Value {
    let output = pnpm(dir)
        .with_args(args)
        .output()
        .expect("run pnpm peers --json");
    assert_eq!(output.status.code(), Some(expected_code), "unexpected exit: {output:?}");
    serde_json::from_slice(&output.stdout).expect("parse peers JSON")
}

#[test]
fn recursive_peers_checks_per_project_lockfiles_when_shared_workspace_lockfile_is_false() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let peer_consumer = root.path().join("peer-consumer");
    fs::create_dir_all(&peer_consumer).expect("create peer-consumer");
    fs::write(
        peer_consumer.join("package.json"),
        serde_json::json!({
            "name": "peer-consumer",
            "version": "1.0.0",
            "peerDependencies": { "missing-peer": "^1.0.0" },
        })
        .to_string(),
    )
    .expect("write peer-consumer manifest");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\nautoInstallPeers: false\nstrictPeerDependencies: false\n",
    )
    .expect("write workspace manifest");

    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(&app_dir).expect("create app dir");
    fs::write(
        app_dir.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "peer-consumer": "file:../../../peer-consumer" },
        })
        .to_string(),
    )
    .expect("write app manifest");

    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert!(!workspace.join("pnpm-lock.yaml").exists());
    assert!(app_dir.join("pnpm-lock.yaml").exists());

    for args in [&["-r", "peers"][..], &["peers", "check"][..], &["--filter", "app", "peers"][..]] {
        let output = pnpm(&workspace)
            .with_args(args)
            .output()
            .expect("run pnpm peers");
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
        assert!(stdout.contains("missing-peer"), "{args:?} stdout:\n{stdout}");
    }

    let recursive_json = run_peers_json(&workspace, &["-r", "peers", "--json"], 1);
    assert!(recursive_json["packages/app"]["missing"]["missing-peer"].is_array());

    let single_json = run_peers_json(&app_dir, &["peers", "--json"], 1);
    assert!(single_json["."]["missing"]["missing-peer"].is_array());

    // A stale root lockfile without a root package.json is ignored when
    // sharedWorkspaceLockfile is false.
    fs::write(workspace.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\nimporters:\n  .: {}\n")
        .expect("write stale root lockfile");
    let with_stale_root = run_peers_json(&workspace, &["-r", "peers", "--json"], 1);
    let projects = with_stale_root.as_object().expect("issues object");
    assert_eq!(projects.len(), 1);
    assert!(projects["packages/app"]["missing"]["missing-peer"].is_array());

    drop(root);
}

#[test]
fn recursive_peers_checks_workspace_links_and_clean_projects_under_dedicated_lockfiles() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\nautoInstallPeers: false\nstrictPeerDependencies: false\n",
    )
    .expect("write workspace manifest");

    let lib_dir = workspace.join("packages/lib");
    fs::create_dir_all(&lib_dir).expect("create lib dir");
    fs::write(
        lib_dir.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "missing-peer": "^1.0.0" },
        })
        .to_string(),
    )
    .expect("write lib manifest");

    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(&app_dir).expect("create app dir");
    fs::write(
        app_dir.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write app manifest");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let json = run_peers_json(&workspace, &["-r", "peers", "--json"], 1);
    assert!(json["packages/app"]["missing"]["missing-peer"].is_array());

    // Filtering to `lib` alone has no dependencies with unmet peers.
    let clean_output = pnpm(&workspace)
        .with_args(["--filter", "lib", "peers"])
        .output()
        .expect("run filtered pnpm peers");
    assert!(clean_output.status.success(), "{clean_output:?}");
    let stdout = String::from_utf8(clean_output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("No peer dependency issues found"), "stdout:\n{stdout}");

    drop(root);
}
