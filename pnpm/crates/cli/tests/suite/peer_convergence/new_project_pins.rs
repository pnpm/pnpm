use super::moving_providers::{
    install_lockfile, normalize_registry_origin, registry_from_manifests,
};
use crate::_utils::{ManifestDeps, WorkspaceFixture, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use serde_json::json;

#[test]
fn new_projects_classify_prior_peer_candidates_from_manifests() {
    let bundle = read_v11_bundle_path();
    for (new_project, suffix_length) in [("a", 1), ("a", 1000), ("zz", 1), ("zz", 1000)] {
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
                    json!({"name":"owner","version":"1.0.0","dependencies":{"provider":"1.0.0"},"peerDependencies":{"provider":"*","auxiliary":"1.0.0"},"peerDependenciesMeta":{"provider":{"optional":true}}}),
                    json!({"name":"consumer","version":"1.0.0","peerDependencies":{"provider":"*"},"peerDependenciesMeta":{"provider":{"optional":true}}}),
                    json!({"name":"provider","version":"1.0.0"}),
                    json!({"name":"provider","version":"2.0.0"}),
                    json!({"name":"auxiliary","version":"1.0.0"}),
                ],
            );
            fixture.append_workspace_yaml(&format!("autoInstallPeers: true\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\npeersSuffixMaxLength: {suffix_length}\nlockfile:\n  format: v9\n"));
            fixture.write_root_manifest("root", ManifestDeps::default());
            fixture.project(
                "z",
                "existing",
                ManifestDeps { prod: &[("owner", "1.0.0")], ..Default::default() },
            );
            fixture.project(
                "versions",
                "versions",
                ManifestDeps { prod: &[("provider", "2.0.0")], ..Default::default() },
            );
            install_lockfile(&fixture, bundle);
            fixture.project(
                new_project,
                "added",
                ManifestDeps { prod: &[("consumer", "1.0.0")], ..Default::default() },
            );
            install_lockfile(&fixture, bundle);
            let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
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
