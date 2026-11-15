use super::{
    moving_providers::{install_lockfile, normalize_registry_origin, registry_from_manifests},
    new_project_pins::peer_provider_version,
};
use crate::_utils::{ManifestDeps, WorkspaceFixture, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use serde_json::json;
use std::fs;

#[test]
fn patched_packages_retain_optional_peer_versions() {
    let bundle = read_v11_bundle_path();
    for suffix_length in [1, 1000] {
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
                    json!({"name":"consumer","version":"1.0.0","peerDependencies":{"provider":"*"},"peerDependenciesMeta":{"provider":{"optional":true}}}),
                    json!({"name":"receiver","version":"1.0.0","peerDependencies":{"provider":"*"},"peerDependenciesMeta":{"provider":{"optional":true}}}),
                    json!({"name":"provider","version":"1.0.0"}),
                    json!({"name":"provider","version":"2.0.0"}),
                ],
            );
            fs::write(fixture.workspace.join("consumer.patch"), crate::patch::MARKER_PATCH)
                .unwrap();
            fixture.append_workspace_yaml(&format!("autoInstallPeers: true\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\npeersSuffixMaxLength: {suffix_length}\nlockfile:\n  format: v9\npatchedDependencies:\n  consumer@1.0.0: consumer.patch\n"));
            fixture.write_root_manifest("root", ManifestDeps::default());
            fixture.project(
                "app",
                "app",
                ManifestDeps { prod: &[("consumer", "1.0.0")], ..Default::default() },
            );
            fixture.project(
                "first",
                "first",
                ManifestDeps { prod: &[("provider", "1.0.0")], ..Default::default() },
            );
            install_lockfile(&fixture, bundle);
            let initial = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            assert_eq!(peer_provider_version(&initial, "packages/app", "consumer"), "1.0.0");
            fixture.project(
                "second",
                "second",
                ManifestDeps { prod: &[("provider", "2.0.0")], ..Default::default() },
            );
            install_lockfile(&fixture, bundle);
            fixture.project(
                "new",
                "new",
                ManifestDeps { prod: &[("receiver", "1.0.0")], ..Default::default() },
            );
            install_lockfile(&fixture, bundle);
            let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            assert_eq!(
                peer_provider_version(&changed, "packages/app", "consumer"),
                "1.0.0",
                "{bundle:?}, suffix={suffix_length}",
            );
            install_lockfile(&fixture, bundle);
            assert_eq!(
                changed,
                Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap(),
            );

            assert_eq!(peer_provider_version(&changed, "packages/new", "receiver"), "1.0.0");
            let normalized = normalize_registry_origin(changed, &fixture);
            if let Some(expected) = &expected {
                assert_eq!(expected, &normalized);
            }
            expected = Some(normalized);
        }
    }
}
