use super::run_peers;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn named_registry_peers_match_their_semver() {
    assert_named_registry_peer("5.x.x", false);
}

#[test]
fn linked_package_peers_match_named_registry_semver() {
    assert_named_registry_peer("5.x.x", true);
}

#[test]
fn named_registry_peer_mismatches_report_semver_and_honor_allowed_versions() {
    assert_named_registry_peer("6.x.x", false);
}

fn assert_named_registry_peer(wanted: &str, linked: bool) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"app"}"#).unwrap();
    let config =
        "namedRegistries:\n  work: https://npm.example.com/\nstrictPeerDependencies: true\n";
    fs::write(workspace.join("pnpm-workspace.yaml"), config).unwrap();
    let sdk_ref = if linked { "link:sdk" } else { "work:0.6.6(@work/adapter@work:5.1.7)" };
    fs::create_dir_all(workspace.join("sdk")).unwrap();
    fs::write(
        workspace.join("sdk/package.json"),
        json!({"name": "@work/sdk", "version": "0.6.6", "peerDependencies": {"@work/adapter": wanted}}).to_string(),
    ).unwrap();
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        format!(
            "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      '@work/sdk':
        specifier: work:0.6.6
        version: {sdk_ref}
      '@work/adapter':
        specifier: work:5.1.7
        version: work:5.1.7
packages:
  '@work/sdk@work:0.6.6':
    resolution: {{tarball: https://npm.example.com/package.tgz}}
    peerDependencies:
      '@work/adapter': '{wanted}'
  '@work/adapter@work:5.1.7':
    resolution: {{tarball: https://npm.example.com/package.tgz}}
snapshots:
  '@work/sdk@work:0.6.6(@work/adapter@work:5.1.7)':
    dependencies:
      '@work/adapter': work:5.1.7
  '@work/adapter@work:5.1.7': {{}}
",
        ),
    )
    .unwrap();
    let output = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["peers", "check", "--lockfile-only", "--json"])
        .output()
        .unwrap();
    let issues: Value = serde_json::from_slice(&output.stdout).expect("parse peer issues");
    dbg!(&issues);
    if wanted == "5.x.x" {
        assert!(output.status.success(), "{output:?}");
        assert_eq!(issues["."]["bad"], json!({}));
        assert_eq!(issues["."]["missing"], json!({}));
    } else {
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let issue = &issues["."]["bad"]["@work/adapter"][0];
        assert_eq!(issue["foundVersion"], "5.1.7");
        assert_eq!(issue["wantedRange"], wanted);
        assert_eq!(issue["parents"][0]["version"], "0.6.6");
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            format!(
                "{config}peerDependencyRules:\n  allowedVersions:\n    '@work/adapter': '5.x.x'\n",
            ),
        )
        .unwrap();
        let allowed = run_peers(&workspace, &["peers", "check", "--lockfile-only", "--json"]);
        dbg!(&allowed);
        assert_eq!(allowed["."]["bad"], json!({}));
    }
    drop(root);
}
