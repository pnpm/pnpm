use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn global_command(workspace: &Path, home: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
        .with_env("PNPM_HOME", home)
        .with_env("XDG_CONFIG_HOME", home.join("config"))
        .with_env("NO_COLOR", "1")
}

// These lockfiles are inspected without fetching or extracting packages.
const TARBALL: &str = "https://example.invalid/fixture.tgz";

fn lockfile(peer_version: Option<&str>) -> Value {
    let mut lockfile = json!({
        "lockfileVersion": "9.0",
        "importers": {".": {"dependencies": {
            "consumer": {"specifier": "1.0.0", "version": "1.0.0"}
        }}},
        "packages": {"consumer@1.0.0": {"resolution": {"tarball": TARBALL}, "peerDependencies": {"peer": "^2.0.0"}}},
        "snapshots": {"consumer@1.0.0": {}}
    });
    if let Some(version) = peer_version {
        lockfile["packages"][format!("peer@{version}")] =
            json!({"resolution": {"tarball": TARBALL}});
        lockfile["snapshots"][format!("peer@{version}")] = json!({});
        lockfile["snapshots"]["consumer@1.0.0"]["dependencies"] = json!({"peer": version});
    }
    lockfile
}

fn write_group(home: &Path, name: &str, aliases: &[&str], lockfile: &Value) -> PathBuf {
    let global_dir = home.join("global/v11");
    let install_dir = global_dir.join(format!("install-{name}"));
    fs::create_dir_all(&install_dir).expect("create global install directory");
    let dependencies: serde_json::Map<String, Value> = aliases
        .iter()
        .map(|alias| {
            ((*alias).to_string(), json!(if *alias == "peer" { "2.0.0" } else { "1.0.0" }))
        })
        .collect();
    fs::write(install_dir.join("package.json"), json!({"dependencies": dependencies}).to_string())
        .expect("write group manifest");
    let mut group_lockfile = lockfile.clone();
    let mut root_dependencies = serde_json::Map::new();
    for alias in aliases {
        let version = if *alias == "peer" { "2.0.0" } else { "1.0.0" };
        root_dependencies.insert(
            (*alias).to_string(),
            json!({"specifier": version, "version": version}),
        );
        group_lockfile["packages"][format!("{alias}@{version}")] =
            json!({"resolution": {"tarball": TARBALL}});
        group_lockfile["snapshots"][format!("{alias}@{version}")] = if *alias == "peer" {
            json!({})
        } else {
            json!({"dependencies": {"consumer": "1.0.0"}})
        };
    }
    group_lockfile["importers"]["."]["dependencies"] = Value::Object(root_dependencies);
    fs::write(install_dir.join("pnpm-lock.yaml"), group_lockfile.to_string())
        .expect("write group lockfile");
    pnpm_fs::symlink_dir(&install_dir, &global_dir.join(format!("hash-{name}")))
        .expect("activate the install group");
    install_dir
}

#[test]
fn global_peers_flags_preserve_the_local_report_format() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let home = root.path().join("home");
    let install = write_group(&home, "tool", &["@scope/tool"], &lockfile(Some("1.0.0")));
    let local = global_command(&install, &home)
        .with_args(["peers", "check"])
        .output()
        .expect("check the individual install");
    assert_eq!(local.status.code(), Some(1));
    let local = String::from_utf8(local.stdout).expect("UTF-8 local output");
    let body = local
        .strip_prefix("Issues with peer dependencies found\n\n")
        .expect("existing peers heading")
        .trim_end();
    let body = body
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let expected = format!("Issues with peer dependencies found\n\n@scope/tool\n{body}\n");
    for flag in ["-g", "--global"] {
        let output = global_command(&workspace, &home)
            .with_args(["peers", "check", flag])
            .output()
            .expect("check global peers");
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

#[test]
fn global_peers_keeps_install_groups_isolated_and_ignores_inactive_slots() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let home = root.path().join("home");
    let missing = write_group(&home, "missing", &["tool-a"], &lockfile(None));
    write_group(&home, "provider", &["peer", "tool-b"], &lockfile(Some("2.0.0")));
    write_group(&home, "bad", &["tool-c"], &lockfile(Some("1.0.0")));
    let orphan = write_group(&home, "orphan", &["old-tool"], &lockfile(None));
    pnpm_fs::remove_symlink_dir(&home.join("global/v11/hash-orphan"))
        .expect("deactivate the old install");
    assert!(orphan.exists());
    pnpm_fs::symlink_dir(&missing, &home.join("global/v11/duplicate"))
        .expect("second link to the same install");
    let broken = write_group(&home, "broken", &["broken-tool"], &lockfile(None));
    fs::remove_dir_all(broken).expect("leave a dangling slot");
    let invalid = write_group(&home, "invalid", &["invalid-tool"], &lockfile(None));
    fs::write(invalid.join("package.json"), "invalid json").unwrap();

    let output = global_command(&workspace, &home)
        .with_args(["peers", "check", "-g", "--json"])
        .output()
        .expect("check global peers as JSON");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let issues: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        issues
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["peer, tool-b", "tool-a", "tool-c"]
    );
    assert_eq!(issues["tool-a"]["missing"]["peer"][0]["wantedRange"], "^2.0.0");
    assert_eq!(issues["peer, tool-b"]["bad"], json!({}));
    assert_eq!(issues["peer, tool-b"]["missing"], json!({}));
    assert_eq!(issues["tool-c"]["bad"]["peer"][0]["foundVersion"], "1.0.0");
}

#[test]
fn global_peers_prefers_each_current_lockfile_and_honors_lockfile_only() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let home = root.path().join("home");
    let install = write_group(&home, "tool", &["tool"], &lockfile(Some("1.0.0")));
    let install_state = install.join("node_modules/.pnpm");
    fs::create_dir_all(&install_state).unwrap();
    fs::write(install_state.join("lock.yaml"), lockfile(Some("2.0.0")).to_string()).unwrap();
    global_command(&workspace, &home)
        .with_args(["peers", "check", "-g"])
        .assert()
        .success()
        .stdout("No peer dependency issues found\n");
    let output = global_command(&workspace, &home)
        .with_args(["peers", "check", "-g", "--lockfile-only"])
        .assert()
        .code(1);
    assert!(String::from_utf8_lossy(&output.get_output().stdout).contains("✕ unmet peer peer"));
}

#[test]
fn global_peers_ignores_the_callers_workspace_and_uses_global_peer_rules() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let home = root.path().join("home");
    write_group(&home, "tool", &["tool"], &lockfile(Some("1.0.0")));
    fs::write(workspace.join("package.json"), r#"{"name":"caller"}"#).unwrap();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages: ['packages/*']\npeerDependencyRules:\n  allowAny: ['peer']\n",
    )
    .unwrap();
    let output = global_command(&workspace, &home)
        .with_args(["peers", "check", "-g"])
        .assert()
        .code(1);
    assert!(String::from_utf8_lossy(&output.get_output().stdout).contains("✕ unmet peer peer"));
    fs::write(home.join("pnpm-workspace.yaml"), "peerDependencyRules:\n  allowAny: ['peer']\n")
        .unwrap();
    global_command(&workspace, &home)
        .with_args(["peers", "check", "-g"])
        .assert()
        .success()
        .stdout("No peer dependency issues found\n");
}

#[test]
fn global_peers_handles_no_installs_and_clean_installs() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let home = root.path().join("home");
    for args in [["peers", "check", "-g"], ["peers", "check", "--global"]] {
        global_command(&workspace, &home)
            .with_args(args)
            .assert()
            .success()
            .stdout("No peer dependency issues found\n");
    }
    global_command(&workspace, &home)
        .with_args(["peers", "-g", "--json"])
        .assert()
        .success()
        .stdout("{}\n");
    write_group(&home, "clean", &["clean-tool"], &lockfile(Some("2.0.0")));
    global_command(&workspace, &home)
        .with_args(["peers", "check", "-g"])
        .assert()
        .success()
        .stdout("No peer dependency issues found\n");
}

#[test]
fn global_peers_reports_a_corrupt_active_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let home = root.path().join("home");
    let install = write_group(&home, "tool", &["tool"], &lockfile(None));
    fs::write(install.join("pnpm-lock.yaml"), "lockfileVersion: [").unwrap();
    let output = global_command(&workspace, &home)
        .with_args(["peers", "check", "-g"])
        .assert()
        .failure();
    assert!(String::from_utf8_lossy(&output.get_output().stderr).contains("load lockfile in"));
}
