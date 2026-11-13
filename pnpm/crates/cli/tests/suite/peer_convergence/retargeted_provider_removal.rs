use super::moving_providers::{install_lockfile, registry_from_manifests};
use crate::_utils::{ManifestDeps, WorkspaceFixture, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use serde_json::json;
use std::{ffi::OsStr, fs};

#[test]
fn removing_a_retargeted_provider_keeps_injected_transitive_peers() {
    if let Some(bundle) = read_v11_bundle_path() {
        check_removal(Some(&bundle), "v9");
    }
    for format in ["v9", "v13"] {
        check_removal(None, format);
    }
}

fn check_removal(bundle: Option<&OsStr>, format: &str) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = registry_from_manifests(
        registry_dir.path(),
        [
            json!({"name":"provider", "version":"1.0.0", "dependencies":{"consumer":"^1.0.0"}}),
            json!({"name":"provider", "version":"2.0.0"}),
            json!({"name":"consumer", "version":"1.0.0", "peerDependencies":{"provider":"^2.0.0"}}),
            json!({"name":"consumer", "version":"2.0.0"}),
        ],
    );
    fixture.append_workspace_yaml(&format!("autoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\ndedupePeerDependents: true\nlockfile:\n  format: {format}\n"));
    fixture.project(
        "shared",
        "shared",
        ManifestDeps { prod: &[("consumer", "^1.0.0")], ..Default::default() },
    );
    let mut root = json!({"name":"root", "version":"1.0.0", "dependencies":{"provider":"^2.0.0", "shared":"workspace:*"}, "dependenciesMeta":{"shared":{"injected":true}}});
    let path = fixture.workspace.join("package.json");
    fs::write(&path, root.to_string()).unwrap();
    install_lockfile(&fixture, bundle);
    for step in 0..3 {
        match step {
            0 => root["dependencies"]["provider"] = json!("^1.0.0"),
            1 => root["dependencies"]["consumer"] = json!("^1.0.0"),
            _ => {
                root["dependencies"]
                    .as_object_mut()
                    .unwrap()
                    .remove("provider");
            }
        }
        fs::write(&path, root.to_string()).unwrap();
        install_lockfile(&fixture, bundle);
        let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        install_lockfile(&fixture, bundle);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        dbg!(&changed, &reused);
        assert_eq!(changed, reused);
    }
}
