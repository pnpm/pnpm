use super::{
    NodeLinker, ProjectEntry, WorkspaceState, WorkspaceStateSettings, get_file_path,
    load_workspace_state, now_millis, update_workspace_state,
};
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use tempfile::tempdir;

#[test]
fn file_path_matches_upstream() {
    let dir = std::path::Path::new("/tmp/example");
    assert_eq!(get_file_path(dir), dir.join("node_modules").join(".pnpm-workspace-state-v1.json"));
}

#[test]
fn write_and_load_round_trip() {
    let tmp = tempdir().expect("create temp dir");
    let workspace_dir = tmp.path();

    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("my-pkg".into()), version: Some("1.2.3".into()) },
    );

    let mut patched = IndexMap::new();
    patched.insert("some-pkg".to_string(), "patches/some-pkg.patch".to_string());

    let state = WorkspaceState {
        last_validated_timestamp: now_millis(),
        projects,
        pnpmfiles: vec![],
        filtered_install: false,
        config_dependencies: None,
        settings: WorkspaceStateSettings {
            auto_install_peers: Some(true),
            dedupe_peer_dependents: Some(true),
            dev: Some(true),
            hoist_pattern: Some(vec!["*".into()]),
            hoist_workspace_packages: Some(true),
            node_linker: Some(NodeLinker::Isolated),
            optional: Some(true),
            patched_dependencies: Some(patched.clone()),
            production: Some(true),
            public_hoist_pattern: Some(vec![]),
            ..Default::default()
        },
    };

    update_workspace_state(workspace_dir, &state).expect("write state");

    let path = get_file_path(workspace_dir);
    assert!(path.is_file(), "state file should exist at {path:?}");

    let on_disk = std::fs::read_to_string(&path).expect("read state");
    assert!(on_disk.ends_with('\n'), "upstream appends a trailing newline");

    let loaded = load_workspace_state(workspace_dir).expect("load state").expect("file present");
    assert_eq!(loaded, state);
}

#[test]
fn load_returns_none_when_missing() {
    let tmp = tempdir().expect("create temp dir");
    let loaded = load_workspace_state(tmp.path()).expect("missing state is not an error");
    assert!(loaded.is_none());
}

#[test]
fn omits_settings_that_are_none() {
    let state = WorkspaceState {
        last_validated_timestamp: 0,
        projects: BTreeMap::new(),
        pnpmfiles: vec![],
        filtered_install: false,
        config_dependencies: None,
        settings: WorkspaceStateSettings { auto_install_peers: Some(true), ..Default::default() },
    };
    let serialized = serde_json::to_string(&state).expect("serialize");
    // Only the populated setting should show up.
    assert!(serialized.contains("\"autoInstallPeers\":true"), "got: {serialized}");
    assert!(!serialized.contains("dedupePeerDependents"), "got: {serialized}");
    assert!(!serialized.contains("nodeLinker"), "got: {serialized}");
    // Top-level optional keys should also be omitted.
    assert!(!serialized.contains("configDependencies"), "got: {serialized}");
}

#[test]
fn node_linker_serializes_lowercase() {
    let value = serde_json::to_value(NodeLinker::Isolated).expect("serialize");
    assert_eq!(value, serde_json::Value::from("isolated"));
    let value = serde_json::to_value(NodeLinker::Hoisted).expect("serialize");
    assert_eq!(value, serde_json::Value::from("hoisted"));
    let value = serde_json::to_value(NodeLinker::Pnp).expect("serialize");
    assert_eq!(value, serde_json::Value::from("pnp"));
}
