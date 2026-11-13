use super::moving_providers::{install_lockfile, registry_from_manifests};
use crate::_utils::{ManifestDeps, WorkspaceFixture, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::fs;

#[test]
fn retained_leaf_provider_keeps_a_hashed_cycle_context() {
    let bundle = read_v11_bundle_path();
    for (bundle, format) in bundle
        .as_deref()
        .map(|bundle| (Some(bundle), "v9"))
        .into_iter()
        .chain([(None, "v9"), (None, "v13")])
    {
        let registry_dir = tempfile::tempdir().unwrap();
        let mut fixture = WorkspaceFixture::new();
        fixture.registry.mock_instance = registry_from_manifests(
            registry_dir.path(),
            [
                json!({"name":"p0","version":"1.0.0","dependencies":{"p1":"^1.0.0"}}),
                json!({"name":"p0","version":"2.0.0","peerDependencies":{"p2":"*"},"peerDependenciesMeta":{"p2":{"optional":true}}}),
                json!({"name":"p1","version":"1.0.0","dependencies":{"p3":"^1.0.0"}}),
                json!({"name":"p1","version":"2.0.0"}),
                json!({"name":"p2","version":"1.0.0","peerDependencies":{"p3":"*"}}),
                json!({"name":"p2","version":"2.0.0","peerDependencies":{"p0":"^2.0.0"}}),
                json!({"name":"p3","version":"1.0.0","dependencies":{"p2":"^2.0.0"},"peerDependencies":{"p1":"*"}}),
                json!({"name":"p3","version":"2.0.0"}),
            ],
        );
        fixture.append_workspace_yaml(&format!("autoInstallPeers: true\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\ndedupePeerDependents: true\npeersSuffixMaxLength: 20\nlockfile:\n  format: {format}\n"));
        fixture.write_root_manifest(
            "root",
            ManifestDeps { prod: &[("p2", "^1.0.0")], ..Default::default() },
        );
        fixture.project(
            "a",
            "app",
            ManifestDeps { prod: &[("p3", "^1.0.0")], ..Default::default() },
        );
        install_lockfile(&fixture, bundle);
        let fresh = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        install_lockfile(&fixture, bundle);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        dbg!(&fresh, &reused);
        assert_eq!(fresh, reused, "{bundle:?}");
    }
}

#[test]
fn removing_a_consumer_keeps_optional_cycle_context() {
    let bundle = read_v11_bundle_path();
    for (bundle, format) in bundle
        .as_deref()
        .map(|bundle| (Some(bundle), "v9"))
        .into_iter()
        .chain([(None, "v9"), (None, "v13")])
    {
        let registry_dir = tempfile::tempdir().unwrap();
        let mut fixture = WorkspaceFixture::new();
        fixture.registry.mock_instance = registry_from_manifests(
            registry_dir.path(),
            [
                json!({"name":"p0","version":"1.0.0","dependencies":{"p1":"^2.0.0"},"peerDependencies":{"p1":"*"},"peerDependenciesMeta":{"p1":{"optional":true}}}),
                json!({"name":"p0","version":"2.0.0","peerDependencies":{"p1":"^1.0.0"}}),
                json!({"name":"p1","version":"1.0.0"}),
                json!({"name":"p1","version":"2.0.0","peerDependencies":{"p2":"*"}}),
                json!({"name":"p2","version":"1.0.0"}),
                json!({"name":"p2","version":"2.0.0","peerDependencies":{"p0":"^1.0.0","p1":"*"},"peerDependenciesMeta":{"p1":{"optional":true}}}),
            ],
        );
        fixture.append_workspace_yaml(&format!("autoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\ndedupePeerDependents: true\nlockfile:\n  format: {format}\n"));
        fixture.write_root_manifest(
            "root",
            ManifestDeps { prod: &[("p0", "^1.0.0")], ..Default::default() },
        );
        fixture.project(
            "a",
            "app",
            ManifestDeps { prod: &[("p0", "^2.0.0")], ..Default::default() },
        );
        fixture.project(
            "b",
            "other",
            ManifestDeps { prod: &[("p2", "^2.0.0")], ..Default::default() },
        );
        install_lockfile(&fixture, bundle);
        for (directory, alias, specifier) in
            [(".", "p1", Some("^1.0.0")), ("packages/a", "p0", Some("^1.0.0")), (".", "p0", None)]
        {
            let path = fixture.workspace.join(directory).join("package.json");
            let mut manifest: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            match specifier {
                Some(specifier) => {
                    manifest["dependencies"][alias] = json!(specifier);
                }
                None => {
                    manifest["dependencies"]
                        .as_object_mut()
                        .unwrap()
                        .remove(alias);
                }
            }
            fs::write(path, manifest.to_string()).unwrap();
            install_lockfile(&fixture, bundle);
            let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            install_lockfile(&fixture, bundle);
            let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            dbg!(&changed, &reused);
            assert_eq!(
                changed.to_yaml_string().unwrap(),
                reused.to_yaml_string().unwrap(),
                "{bundle:?} {directory} {alias}",
            );
        }
    }
}

#[test]
fn optional_cycle_peer_keeps_its_transitive_context() {
    let bundle = read_v11_bundle_path();
    for (bundle, format) in bundle
        .as_deref()
        .map(|bundle| (Some(bundle), "v9"))
        .into_iter()
        .chain([(None, "v9"), (None, "v13")])
    {
        let registry_dir = tempfile::tempdir().unwrap();
        let mut fixture = WorkspaceFixture::new();
        fixture.registry.mock_instance = registry_from_manifests(
            registry_dir.path(),
            [
                json!({"name":"p0","version":"1.0.0","dependencies":{"p2":"^1.0.0"}}),
                json!({"name":"p0","version":"2.0.0","peerDependencies":{"p1":"*"}}),
                json!({"name":"p1","version":"1.0.0","peerDependencies":{"p4":"^1.0.0"}}),
                json!({"name":"p1","version":"2.0.0"}),
                json!({"name":"p2","version":"1.0.0","dependencies":{"p0":"^2.0.0"},"peerDependencies":{"p0":"^1.0.0"},"peerDependenciesMeta":{"p0":{"optional":true}}}),
                json!({"name":"p2","version":"2.0.0"}),
                json!({"name":"p3","version":"1.0.0"}),
                json!({"name":"p3","version":"2.0.0"}),
                json!({"name":"p4","version":"1.0.0","peerDependencies":{"p0":"*"},"peerDependenciesMeta":{"p0":{"optional":true}}}),
                json!({"name":"p4","version":"2.0.0"}),
            ],
        );
        fixture.append_workspace_yaml(&format!("autoInstallPeers: true\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\ndedupePeerDependents: true\npeersSuffixMaxLength: 20\nlockfile:\n  format: {format}\n"));
        fixture.write_root_manifest(
            "root",
            ManifestDeps { prod: &[("p0", "^1.0.0")], ..Default::default() },
        );
        fixture.project(
            "a",
            "app",
            ManifestDeps { prod: &[("p2", "^1.0.0")], ..Default::default() },
        );
        fixture.project(
            "b",
            "other",
            ManifestDeps { prod: &[("p1", "^1.0.0")], ..Default::default() },
        );
        install_lockfile(&fixture, bundle);
        let fresh = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        install_lockfile(&fixture, bundle);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        dbg!(&fresh, &reused);
        assert_eq!(
            fresh.to_yaml_string().unwrap(),
            reused.to_yaml_string().unwrap(),
            "{bundle:?} {format}",
        );
    }
}
