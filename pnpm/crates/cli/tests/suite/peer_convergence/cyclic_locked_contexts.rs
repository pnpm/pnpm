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
        let (_registry_dir, fixture) = retained_cycle_fixture(format);
        install_lockfile(&fixture, bundle);
        let fresh = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        install_lockfile(&fixture, bundle);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        dbg!(&fresh, &reused);
        assert_eq!(fresh, reused, "{bundle:?}");
    }
}

fn retained_cycle_fixture(format: &str) -> (tempfile::TempDir, WorkspaceFixture) {
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
    fixture.project("a", "app", ManifestDeps { prod: &[("p3", "^1.0.0")], ..Default::default() });
    (registry_dir, fixture)
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

#[test]
fn explicit_cycle_provider_stays_with_its_importer() {
    let bundle = read_v11_bundle_path();
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for (bundle, format) in bundle
        .as_deref()
        .map(|bundle| (Some(bundle), "v9"))
        .into_iter()
        .chain([(None, "v9"), (None, "v13")])
    {
        for (dedupe, suffix_length) in [(false, 1), (false, 1000), (true, 1), (true, 1000)] {
            let (_registry_dir, fixture) = retained_cycle_fixture(format);
            let config_path = fixture.workspace.join("pnpm-workspace.yaml");
            let config = fs::read_to_string(&config_path)
                .unwrap()
                .replace("dedupePeerDependents: true", &format!("dedupePeerDependents: {dedupe}"))
                .replace(
                    "peersSuffixMaxLength: 20",
                    &format!("peersSuffixMaxLength: {suffix_length}"),
                );
            fs::write(config_path, config).unwrap();
            fixture.project(
                "a",
                "app",
                ManifestDeps { prod: &[("p3", "^1.0.0"), ("p2", "2.0.0")], ..Default::default() },
            );
            install_lockfile(&fixture, bundle);
            let lockfile = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            install_lockfile(&fixture, bundle);
            let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            assert_eq!(lockfile, reused);
            for (importer, version) in [(".", "1.0.0"), ("packages/a", "2.0.0")] {
                let label = format!("{bundle:?} {format} {dedupe} {suffix_length} {importer}");
                actual.push((label.clone(), cycle_provider(&lockfile, importer)));
                expected.push((label, format!("p2@{version}")));
            }
        }
    }
    assert_eq!(actual, expected);
}

fn cycle_provider(lockfile: &Lockfile, importer: &str) -> String {
    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let alias = if importer == "." { "p2" } else { "p3" }.parse().unwrap();
    let mut key = lockfile.importers[importer].dependencies.as_ref().unwrap()[&alias]
        .version
        .resolved_key(&alias)
        .unwrap();
    let aliases: &[&str] = if importer == "." { &["p3", "p2", "p0"] } else { &["p2", "p0"] };
    for alias in aliases {
        let alias = alias.parse().unwrap();
        key = snapshots[&key].dependencies.as_ref().unwrap()[&alias].resolve(&alias).unwrap();
    }
    let alias = "p2".parse().unwrap();
    snapshots[&key].optional_dependencies.as_ref().unwrap()[&alias]
        .resolve(&alias)
        .unwrap()
        .without_peer()
        .to_string()
}

#[test]
#[ignore = "requires PNPM_V11_BUNDLE; run by the peer convergence CI job"]
fn fresh_hashed_cycle_lockfiles_match() {
    use crate::peer_convergence::moving_providers::normalize_registry_origin;

    let bundle =
        read_v11_bundle_path().expect("PNPM_V11_BUNDLE is required for the cycle comparison");
    let mut outputs = Vec::new();
    for cli in [Some(bundle.as_os_str()), None] {
        let (_registry_dir, fixture) = retained_cycle_fixture("v9");
        install_lockfile(&fixture, cli);
        let lockfile = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        outputs.push(normalize_registry_origin(lockfile, &fixture).to_yaml_string().unwrap());
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
#[ignore = "requires PNPM_V11_BUNDLE; run by the peer convergence CI job"]
fn aliased_cycle_provider_lockfiles_match() {
    use crate::peer_convergence::moving_providers::normalize_registry_origin;

    let bundle =
        read_v11_bundle_path().expect("PNPM_V11_BUNDLE is required for the cycle comparison");
    let mut outputs = Vec::new();
    for cli in [Some(bundle.as_os_str()), None] {
        let registry_dir = tempfile::tempdir().unwrap();
        let mut fixture = WorkspaceFixture::new();
        fixture.registry.mock_instance = registry_from_manifests(
            registry_dir.path(),
            [
                json!({"name":"provider","version":"1.0.0","dependencies":{"consumer":"1.0.0"}}),
                json!({"name":"consumer","version":"1.0.0","peerDependencies":{"alias":"*"}}),
            ],
        );
        fixture.append_workspace_yaml(
            "autoInstallPeers: false\ndedupePeers: false\nlockfile:\n  format: v9\n",
        );
        fixture.write_root_manifest(
            "root",
            ManifestDeps { prod: &[("alias", "npm:provider@1.0.0")], ..Default::default() },
        );
        install_lockfile(&fixture, cli);
        let lockfile = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        install_lockfile(&fixture, cli);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        assert_eq!(lockfile, reused);
        outputs.push(normalize_registry_origin(lockfile, &fixture).to_yaml_string().unwrap());
    }
    assert_eq!(outputs[0], outputs[1]);
}
