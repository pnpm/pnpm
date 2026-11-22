use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::Command,
};

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

    let clean_output = pnpm(&workspace)
        .with_args(["--filter", "lib", "peers"])
        .output()
        .expect("run filtered pnpm peers");
    assert!(clean_output.status.success(), "{clean_output:?}");
    let stdout = String::from_utf8(clean_output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("No peer dependency issues found"), "stdout:\n{stdout}");

    drop(root);
}

#[test]
fn dedicated_lockfiles_satisfy_linked_sibling_peers_from_the_siblings_own_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let peer_provider = workspace.join("vendor/peer-provider");
    fs::create_dir_all(&peer_provider).expect("create peer-provider");
    fs::write(
        peer_provider.join("package.json"),
        serde_json::json!({ "name": "peer-provider", "version": "1.2.0" }).to_string(),
    )
    .expect("write peer-provider manifest");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - packages/nested/*\nsharedWorkspaceLockfile: false\nautoInstallPeers: false\nstrictPeerDependencies: false\n",
    )
    .expect("write workspace manifest");

    let lib_dir = workspace.join("packages/nested/lib");
    fs::create_dir_all(&lib_dir).expect("create lib dir");
    fs::write(
        lib_dir.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "peer-provider": "^1.0.0" },
            "dependencies": { "peer-provider": "file:../../../vendor/peer-provider" },
        })
        .to_string(),
    )
    .expect("write lib manifest");

    let peerless_dir = workspace.join("packages/peerless");
    fs::create_dir_all(&peerless_dir).expect("create peerless dir");
    fs::write(
        peerless_dir.join("package.json"),
        serde_json::json!({ "name": "peerless", "version": "1.0.0" }).to_string(),
    )
    .expect("write peerless manifest");

    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(&app_dir).expect("create app dir");
    fs::write(
        app_dir.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "lib": "workspace:*",
                "peerless": "workspace:*",
            },
        })
        .to_string(),
    )
    .expect("write app manifest");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    for (dir, args) in [
        (&workspace, &["-r", "peers"][..]),
        (&workspace, &["--filter", "app", "peers"][..]),
        (&app_dir, &["peers"][..]),
    ] {
        let output = pnpm(dir)
            .with_args(args)
            .output()
            .expect("run pnpm peers");
        assert!(output.status.success(), "{args:?}: {output:?}");
        let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
        assert!(stdout.contains("No peer dependency issues found"), "{args:?} stdout:\n{stdout}");
    }

    fs::write(peerless_dir.join("pnpm-lock.yaml"), ": invalid: [yaml")
        .expect("corrupt peerless lockfile");
    let peerless_state = peerless_dir.join("node_modules/.pnpm/lock.yaml");
    if peerless_state.exists() {
        fs::remove_file(peerless_state).expect("remove peerless current lockfile");
    }
    let filtered = pnpm(&workspace)
        .with_args(["--filter", "app", "peers"])
        .output()
        .expect("run filtered pnpm peers with peerless corrupt lockfile");
    assert!(filtered.status.success(), "{filtered:?}");

    drop(root);
}

#[test]
fn install_fails_on_a_missing_peer_of_a_linked_sibling_under_dedicated_lockfiles() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\nautoInstallPeers: false\nstrictPeerDependencies: true\n",
    )
    .expect("write workspace manifest");
    for (dir, manifest) in [
        (
            "lib",
            serde_json::json!({
                "name": "lib",
                "version": "1.0.0",
                "peerDependencies": { "missing-peer": "^1.0.0" },
            }),
        ),
        (
            "app",
            serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "lib": "workspace:*" },
            }),
        ),
    ] {
        let dir = workspace.join("packages").join(dir);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write manifest");
    }

    let output = pacquet
        .with_arg("install")
        .output()
        .expect("run pnpm install");
    assert!(!output.status.success(), "the missing peer must fail the install: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PEER_DEP_ISSUES"), "{stderr}");
    assert!(stderr.contains("missing-peer"), "{stderr}");

    drop(root);
}

#[test]
fn install_accepts_a_peer_the_linked_sibling_provides_from_its_own_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\nautoInstallPeers: false\nstrictPeerDependencies: true\n",
    )
    .expect("write workspace manifest");
    let provider = workspace.join("provider");
    fs::create_dir_all(&provider).expect("create provider dir");
    fs::write(provider.join("package.json"), r#"{"name":"provider","version":"1.0.0"}"#)
        .expect("write provider manifest");
    for (dir, manifest) in [
        (
            "lib",
            serde_json::json!({
                "name": "lib",
                "version": "1.0.0",
                "peerDependencies": { "provider": "^1.0.0" },
                "devDependencies": { "provider": "link:../../provider" },
            }),
        ),
        (
            "app",
            serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "lib": "workspace:*" },
            }),
        ),
    ] {
        let dir = workspace.join("packages").join(dir);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write manifest");
    }

    pacquet
        .with_arg("install")
        .assert()
        .success();

    drop(root);
}

#[test]
fn install_ignores_an_unreadable_sibling_lockfile_when_the_consumer_provides_the_peer() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\nautoInstallPeers: false\nstrictPeerDependencies: true\n",
    )
    .expect("write workspace manifest");
    let provider = workspace.join("provider");
    fs::create_dir_all(&provider).expect("create provider dir");
    fs::write(provider.join("package.json"), r#"{"name":"provider","version":"1.0.0"}"#)
        .expect("write provider manifest");
    for (dir, manifest) in [
        (
            "lib",
            serde_json::json!({
                "name": "lib",
                "version": "1.0.0",
                "peerDependencies": { "provider": "^1.0.0" },
            }),
        ),
        (
            "app",
            serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "lib": "workspace:*", "provider": "link:../../provider" },
            }),
        ),
    ] {
        let dir = workspace.join("packages").join(dir);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write manifest");
    }
    fs::write(workspace.join("packages/lib/pnpm-lock.yaml"), "lockfileVersion: [\n")
        .expect("write an unreadable sibling lockfile");

    pacquet
        .with_args(["--filter", "app", "install"])
        .assert()
        .success();

    drop(root);
}

#[test]
fn lockfile_only_install_reads_the_wanted_lockfile_of_a_linked_sibling() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\nautoInstallPeers: false\nstrictPeerDependencies: true\n",
    )
    .expect("write workspace manifest");
    for version in ["1.0.0", "2.0.0"] {
        let provider = workspace.join(format!("provider-{version}"));
        fs::create_dir_all(&provider).expect("create provider dir");
        fs::write(
            provider.join("package.json"),
            serde_json::json!({ "name": "provider", "version": version }).to_string(),
        )
        .expect("write provider manifest");
    }
    let lib = workspace.join("packages/lib");
    let write_lib = |peer_range: &str, provider_dir: &str| {
        fs::write(
            lib.join("package.json"),
            serde_json::json!({
                "name": "lib",
                "version": "1.0.0",
                "peerDependencies": { "provider": peer_range },
                "devDependencies": { "provider": format!("link:../../{provider_dir}") },
            })
            .to_string(),
        )
        .expect("write lib manifest");
    };
    fs::create_dir_all(&lib).expect("create lib dir");
    write_lib("^1.0.0", "provider-1.0.0");
    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create app dir");
    fs::write(
        app.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"lib":"workspace:*"}}"#,
    )
    .expect("write app manifest");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    write_lib("^2.0.0", "provider-2.0.0");
    // A changed manifest makes the run re-resolve `app`, which is what
    // reports its peer issues.
    let other = workspace.join("other");
    fs::create_dir_all(&other).expect("create other dir");
    fs::write(other.join("package.json"), r#"{"name":"other","version":"1.0.0"}"#)
        .expect("write other manifest");
    fs::write(
        app.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"lib":"workspace:*","other":"link:../../other"}}"#,
    )
    .expect("rewrite app manifest");
    pnpm(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    drop(root);
}
