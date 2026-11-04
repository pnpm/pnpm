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

fn assert_no_warnings(
    inspector: &mut UndeclaredPeerInspector<'_>,
    importer: &str,
    root_scope: crate::WorkspaceRootScope,
) {
    let warnings = inspector.check(importer, root_scope);
    assert!(warnings.is_empty(), "{warnings:?}");
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
    let declared = lockfile(value.clone());
    let mut inspector = UndeclaredPeerInspector::new(&declared, root.path());
    assert_no_warnings(&mut inspector, "packages/app", crate::WorkspaceRootScope::Exclude);
    value["importers"]["packages/app"]["dependencies"]
        .as_object_mut()
        .unwrap()
        .remove("provider");
    value["importers"]["."] = json!({"dependencies":{"alias":{"specifier":"workspace:*", "version":"link:packages/provider"}}});
    let aliased_root = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&aliased_root, root.path());
    assert_no_warnings(&mut inspector, "packages/app", crate::WorkspaceRootScope::Include);
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
    assert_no_warnings(&mut inspector, "packages/app", crate::WorkspaceRootScope::Include);
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

#[test]
fn direct_and_ancestor_aliases_provide_the_peer_by_its_package_name() {
    let mut value = fixture();
    value["importers"]["."]["dependencies"]["alias"] =
        json!({"specifier":"npm:provider@1.0.0", "version":"provider@1.0.0"});
    let direct = lockfile(value.clone());
    let mut inspector = UndeclaredPeerInspector::new(&direct, Path::new("."));
    assert_no_warnings(&mut inspector, ".", crate::WorkspaceRootScope::Exclude);
    value["packages"]["parent@1.0.0"] = json!({"resolution":{"integrity":"sha512-parent"}});
    value["snapshots"]["parent@1.0.0"] =
        json!({"dependencies":{"consumer":"1.0.0", "alias":"provider@1.0.0"}});
    value["importers"]["."]["dependencies"] =
        json!({"parent":{"specifier":"1.0.0", "version":"1.0.0"}});
    let ancestor = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&ancestor, Path::new("."));
    assert_no_warnings(&mut inspector, ".", crate::WorkspaceRootScope::Exclude);
}

#[test]
fn independent_peer_targets_do_not_search_unrelated_roots() {
    let mut value = fixture();
    value["importers"]["."]["dependencies"] = json!({});
    for index in 0..1000 {
        let consumer = format!("consumer{index}");
        let provider = format!("provider{index}");
        value["importers"]["."]["dependencies"][&consumer] =
            json!({"specifier":"1.0.0", "version":"1.0.0"});
        value["packages"][format!("{consumer}@1.0.0")] = json!({"resolution":{"integrity":"sha512-consumer"}, "peerDependencies":{&provider:"*"}});
        value["packages"][format!("{provider}@1.0.0")] =
            json!({"resolution":{"integrity":"sha512-provider"}});
        value["snapshots"][format!("{consumer}@1.0.0")] =
            json!({"dependencies":{&provider:"1.0.0"}});
        value["snapshots"][format!("{provider}@1.0.0")] = json!({});
    }
    let lockfile = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&lockfile, Path::new("."));
    assert_eq!(inspector.check(".", crate::WorkspaceRootScope::Exclude).len(), 1000);
    assert!(
        inspector.visited_states.get() <= 2000,
        "{} ancestor visits",
        inspector.visited_states.get(),
    );
}

#[test]
fn a_shared_peer_is_checked_once_through_a_long_chain() {
    let mut value = fixture();
    value["importers"]["."]["dependencies"] =
        json!({"consumer0":{"specifier":"1.0.0", "version":"1.0.0"}});
    for index in 0..1000 {
        let key = format!("consumer{index}@1.0.0");
        value["packages"][&key] = json!({"resolution":{"integrity":"sha512-consumer"}, "peerDependencies":{"provider":"*"}});
        value["snapshots"][&key] = json!({"dependencies":{"provider":"1.0.0"}});
        if index < 999 {
            value["snapshots"][&key]["dependencies"][format!("consumer{}", index + 1)] =
                "1.0.0".into();
        }
    }
    let lockfile = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&lockfile, Path::new("."));
    assert_eq!(inspector.check(".", crate::WorkspaceRootScope::Exclude).len(), 1000);
    assert!(
        inspector.visited_states.get() <= 2000,
        "{} ancestor visits",
        inspector.visited_states.get(),
    );
}

#[test]
fn declarations_match_provider_versions_across_peer_contexts() {
    let mut value = fixture();
    value["importers"]["."]["dependencies"]["provider"] =
        json!({"specifier":"1.0.0", "version":"1.0.0"});
    value["snapshots"]["consumer@1.0.0"]["dependencies"]["provider"] = "1.0.0(other@1.0.0)".into();
    value["packages"]["provider@1.0.0"]["peerDependencies"] = json!({"other":"*"});
    value["packages"]["provider@1.0.0"]["peerDependenciesMeta"] =
        json!({"other":{"optional":true}});
    value["snapshots"]["provider@1.0.0(other@1.0.0)"] = json!({"dependencies":{"other":"1.0.0"}});
    value["packages"]["other@1.0.0"] = json!({"resolution":{"integrity":"sha512-other"}});
    value["snapshots"]["other@1.0.0"] = json!({});
    let direct = lockfile(value.clone());
    let mut inspector = UndeclaredPeerInspector::new(&direct, Path::new("."));
    assert_no_warnings(&mut inspector, ".", crate::WorkspaceRootScope::Exclude);
    value["packages"]["parent@1.0.0"] = json!({"resolution":{"integrity":"sha512-parent"}});
    value["snapshots"]["parent@1.0.0"] =
        json!({"dependencies":{"consumer":"1.0.0", "provider":"1.0.0"}});
    value["importers"]["."]["dependencies"] =
        json!({"parent":{"specifier":"1.0.0", "version":"1.0.0"}});
    let ancestor = lockfile(value.clone());
    let mut inspector = UndeclaredPeerInspector::new(&ancestor, Path::new("."));
    assert_no_warnings(&mut inspector, ".", crate::WorkspaceRootScope::Exclude);
    value["snapshots"]["parent@1.0.0"]["dependencies"]["provider"] = "2.0.0".into();
    value["packages"]["provider@2.0.0"] = json!({"resolution":{"integrity":"sha512-provider-two"}});
    value["snapshots"]["provider@2.0.0"] = json!({});
    let different_version = lockfile(value);
    let mut inspector = UndeclaredPeerInspector::new(&different_version, Path::new("."));
    let warnings = inspector.check(".", crate::WorkspaceRootScope::Exclude);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].peer, "provider");
}
