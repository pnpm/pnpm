use super::moving_providers::{
    install_lockfile, normalize_registry_origin, registry_from_manifests,
};
use crate::_utils::{ManifestDeps, WorkspaceFixture, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use serde_json::json;

#[test]
fn nested_peer_rebinds_consistently_after_another_project_adds_a_provider() {
    let bundle = read_v11_bundle_path();
    for peers_suffix_max_length in [1, 1000] {
        let mut expected = None;
        for bundle in bundle
            .as_deref()
            .map(Some)
            .into_iter()
            .chain([None])
        {
            let registry_dir = tempfile::tempdir().unwrap();
            let mut fixture = WorkspaceFixture::new();
            fixture.registry.mock_instance = registry_from_manifests(
                registry_dir.path(),
                [
                    json!({"name":"consumer","version":"1.0.0","peerDependencies":{"provider":"1.0.0"}}),
                    json!({"name":"provider","version":"1.0.0","peerDependencies":{"nested":"*"}}),
                    json!({"name":"nested","version":"1.0.0"}),
                    json!({"name":"nested","version":"2.0.0"}),
                ],
            );
            fixture.append_workspace_yaml(&format!("autoInstallPeers: true\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\npeersSuffixMaxLength: {peers_suffix_max_length}\nlockfile:\n  format: v9\n"));
            fixture.write_root_manifest(
                "root",
                ManifestDeps { prod: &[("consumer", "1.0.0")], ..Default::default() },
            );
            fixture.project(
                "a",
                "first",
                ManifestDeps { prod: &[("nested", "1.0.0")], ..Default::default() },
            );
            install_lockfile(&fixture, bundle);
            let initial = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            fixture.project(
                "b",
                "second",
                ManifestDeps { prod: &[("nested", "2.0.0")], ..Default::default() },
            );
            install_lockfile(&fixture, bundle);
            let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            assert_eq!(nested_provider_version(&initial), "1.0.0");
            assert_eq!(nested_provider_version(&changed), "2.0.0");
            install_lockfile(&fixture, bundle);
            assert_eq!(
                changed,
                Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap(),
            );
            let normalized = normalize_registry_origin(changed, &fixture);
            if let Some(expected) = &expected {
                assert_eq!(expected, &normalized);
            }
            expected = Some(normalized);
        }
    }
}

fn nested_provider_version(lockfile: &Lockfile) -> String {
    let consumer = "consumer".parse().unwrap();
    let consumer_key = lockfile.importers["."].dependencies.as_ref().unwrap()[&consumer]
        .version
        .resolved_key(&consumer)
        .unwrap();
    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let provider = "provider".parse().unwrap();
    let provider_key = snapshots[&consumer_key].dependencies.as_ref().unwrap()[&provider]
        .resolve(&provider)
        .unwrap();
    snapshots[&provider_key].dependencies.as_ref().unwrap()[&"nested".parse().unwrap()].to_string()
}
