use super::UndeclaredPeerInspector;
use pnpm_lockfile::Lockfile;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn fixture() -> Value {
    json!({
        "lockfileVersion": "9.0",
        "importers": { ".": { "dependencies": {
            "consumer": { "specifier": "1.0.0", "version": "1.0.0" }
        } } },
        "packages": {
            "consumer@1.0.0": { "resolution": { "integrity": "sha512-consumer" }, "peerDependencies": { "provider": "^1.0.0" } },
            "provider@1.0.0": { "resolution": { "integrity": "sha512-provider" } }
        },
        "snapshots": {
            "consumer@1.0.0": { "dependencies": { "provider": "1.0.0" } },
            "provider@1.0.0": {}
        }
    })
}

fn lockfile(value: Value) -> Lockfile {
    serde_json::from_value(value).expect("parse lockfile fixture")
}

#[test]
fn linked_providers_use_the_lockfile_directory_and_importer_declarations() {
    let root = tempfile::tempdir().expect("create workspace");
    fs::create_dir_all(root.path().join("packages/app")).expect("create consumer directory");
    let provider = root.path().join("packages/provider");
    fs::create_dir_all(&provider).expect("create provider directory");
    fs::write(provider.join("package.json"), r#"{"name":"provider","version":"1.0.0"}"#)
        .expect("write provider manifest");
    let mut value = fixture();
    value["snapshots"]["consumer@1.0.0"]["dependencies"]["provider"] =
        "link:packages/provider".into();
    let undeclared = lockfile(value.clone());
    let mut inspector = UndeclaredPeerInspector::new(&undeclared, root.path());
    assert_eq!(inspector.check(".", crate::WorkspaceRootScope::Exclude).len(), 1);
    value["importers"]["packages/app"] = value["importers"]["."].clone();
    value["importers"]["packages/app"]["dependencies"]["provider"] =
        json!({"specifier":"workspace:*", "version":"link:../provider"});
    let declared = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&declared, root.path());
    assert!(
        inspector.check("packages/app", crate::WorkspaceRootScope::Exclude).is_empty(),
        "declared linked provider must suppress the warning",
    );
}

#[test]
fn workspace_root_aliases_follow_the_resolver_provider_policy() {
    let mut value = fixture();
    value["importers"]["packages/app"] = value["importers"]["."].clone();
    value["importers"]["."] = json!({ "dependencies": {
        "alias": { "specifier": "npm:provider@1.0.0", "version": "provider@1.0.0" }
    }});
    let lockfile = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&lockfile, Path::new("."));
    assert_eq!(inspector.check("packages/app", crate::WorkspaceRootScope::Exclude).len(), 1);
    assert!(
        inspector.check("packages/app", crate::WorkspaceRootScope::Include).is_empty(),
        "the enabled root scope provides the aliased package",
    );
}

#[test]
fn peers_of_reused_peer_packages_are_inspected() {
    let mut value = fixture();
    value["packages"]["provider@1.0.0"]["peerDependencies"] = json!({"other":"*"});
    value["packages"]["other@1.0.0"] = json!({"resolution":{"integrity":"sha512-other"}});
    value["snapshots"]["provider@1.0.0"] = json!({"dependencies":{"other":"1.0.0"}});
    value["snapshots"]["other@1.0.0"] = json!({});
    let lockfile = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&lockfile, Path::new("."));
    let warnings = inspector.check(".", crate::WorkspaceRootScope::Exclude);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn identical_project_scopes_share_the_analysis() {
    let mut value = fixture();
    value["importers"]["packages/another"] = value["importers"]["."].clone();
    let lockfile = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&lockfile, Path::new("."));
    assert_eq!(inspector.check(".", crate::WorkspaceRootScope::Exclude).len(), 1);
    assert_eq!(inspector.check("packages/another", crate::WorkspaceRootScope::Exclude).len(), 1);
    assert_eq!(inspector.reports.len(), 1);
    assert_eq!(inspector.nodes.len(), 2);
}

#[test]
fn converging_branches_do_not_multiply_provider_scopes() {
    let mut value = fixture();
    value["importers"]["."]["dependencies"] =
        json!({"join0":{"specifier":"1.0.0", "version":"1.0.0"}});
    for level in 0..24 {
        let next = if level == 23 { "consumer".to_string() } else { format!("join{}", level + 1) };
        let left = format!("left{level}");
        let right = format!("right{level}");
        let join = format!("join{level}");
        for name in [&left, &right, &join] {
            value["packages"][format!("{name}@1.0.0")] =
                json!({"resolution":{"integrity":"sha512-fixture"}});
        }
        value["snapshots"][format!("{join}@1.0.0")] =
            json!({"dependencies":{&left:"1.0.0", &right:"1.0.0"}});
        value["snapshots"][format!("{left}@1.0.0")] =
            json!({"dependencies":{&next:"1.0.0", format!("marker{level}"):"1.0.0"}});
        value["snapshots"][format!("{right}@1.0.0")] = json!({"dependencies":{&next:"1.0.0"}});
    }
    let lockfile = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&lockfile, Path::new("."));
    let warnings = inspector.check(".", crate::WorkspaceRootScope::Exclude);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(inspector.nodes.len() < 100, "{} analyzed nodes", inspector.nodes.len());
}
