use super::moving_providers::registry_from_manifests;
use crate::_utils::{ManifestDeps, WorkspaceFixture, assert_success, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use serde_json::json;
use std::{ffi::OsStr, fs};

#[test]
fn unchanged_install_retains_a_reachable_locked_peer_provider() {
    for direct in [false, true] {
        for length in [20, 1000] {
            for format in ["v9", "v13"] {
                check_locked_provider(None, format, length, direct);
            }
            if let Some(bundle) = read_v11_bundle_path() {
                check_locked_provider(Some(&bundle), "v9", length, direct);
            }
        }
    }
}

fn check_locked_provider(bundle: Option<&OsStr>, format: &str, length: usize, direct: bool) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = registry_from_manifests(
        registry_dir.path(),
        [
            json!({"name":"peer", "version":"1.0.0"}),
            json!({"name":"peer", "version":"2.0.0"}),
            json!({"name":"retainer", "version":"1.0.0", "dependencies":{"peer":"2.0.0"}}),
            json!({"name":"holder", "version":"1.0.0", "dependencies":{"peer":"1.0.0"}}),
            json!({"name":"wrapper", "version":"1.0.0", "dependencies":{"consumer":"1.0.0", "peer":"1.0.0"}}),
            json!({"name":"consumer", "version":"1.0.0", "peerDependencies":{"peer":">=1"}}),
        ],
    );
    fixture.append_workspace_yaml(&format!("autoInstallPeers: false\ndedupePeerDependents: false\npeersSuffixMaxLength: {length}\nlockfile:\n  format: {format}\n"));
    fixture.write_root_manifest(
        "root",
        ManifestDeps {
            prod: if direct {
                &[("retainer", "1.0.0"), ("peer", "1.0.0"), ("consumer", "1.0.0")]
            } else {
                &[("retainer", "1.0.0"), ("holder", "1.0.0"), ("wrapper", "1.0.0")]
            },
            ..Default::default()
        },
    );
    install(&fixture, bundle, &["install", "--no-prefer-frozen-lockfile"]);
    let mut lockfile = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    assert_consumer_provider(&lockfile, "1.0.0");
    let snapshots = lockfile.snapshots.as_mut().unwrap();
    let old = "consumer@1.0.0(peer@1.0.0)".parse().unwrap();
    let mut consumer = snapshots.remove(&old).unwrap();
    consumer.dependencies
        .as_mut()
        .unwrap()
        .insert("peer".parse().unwrap(), "2.0.0".parse().unwrap());
    snapshots.insert("consumer@1.0.0(peer@2.0.0)".parse().unwrap(), consumer);
    if direct {
        lockfile.importers
            .get_mut(".")
            .unwrap()
            .dependencies
            .as_mut()
            .unwrap()
            .get_mut(&"consumer".parse().unwrap())
            .unwrap()
            .version = "1.0.0(peer@2.0.0)".parse().unwrap();
    } else {
        snapshots
            .get_mut(&"wrapper@1.0.0".parse().unwrap())
            .unwrap()
            .dependencies
            .as_mut()
            .unwrap()
            .insert("consumer".parse().unwrap(), "1.0.0(peer@2.0.0)".parse().unwrap());
    }
    fs::write(fixture.workspace.join("pnpm-lock.yaml"), lockfile.to_yaml_string().unwrap())
        .unwrap();
    for _ in 0..2 {
        install(&fixture, bundle, &["install", "--no-prefer-frozen-lockfile"]);
        let actual = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        assert_consumer_provider(&actual, "2.0.0");
    }
    install(&fixture, bundle, &["install", "--frozen-lockfile"]);
    if direct {
        let retained = fs::read(fixture.workspace.join("pnpm-lock.yaml")).unwrap();
        install(&fixture, bundle, &["update", "peer", "--lockfile-only"]);
        let updated = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        assert_consumer_provider(&updated, "1.0.0");
        fs::write(fixture.workspace.join("pnpm-lock.yaml"), retained).unwrap();
    }
    install(
        &fixture,
        bundle,
        &["add", "peer@1.0.0", "--lockfile-only", "--ignore-workspace-root-check"],
    );
    let updated = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    assert_consumer_provider(&updated, "1.0.0");
}

fn assert_consumer_provider(lockfile: &Lockfile, version: &str) {
    let consumers: Vec<_> = lockfile.snapshots
        .iter()
        .flatten()
        .filter(|(key, _)| key.name.to_string() == "consumer")
        .collect();
    assert!(!consumers.is_empty());
    for (_, snapshot) in consumers {
        assert_eq!(
            snapshot.dependencies.as_ref().unwrap()[&"peer".parse().unwrap()].to_string(),
            version,
        );
    }
}

fn install(fixture: &WorkspaceFixture, bundle: Option<&OsStr>, args: &[&str]) {
    let output = match bundle {
        Some(bundle) => fixture.v11_command_at(bundle, &fixture.workspace, args),
        None => fixture.command_at(&fixture.workspace, args),
    };
    assert_success(&output);
}

#[test]
fn removing_injected_consumers_keeps_locked_cyclic_peers_stable() {
    for format in ["v9", "v13"] {
        check_removed_injected_consumers(None, format);
    }
    if let Some(bundle) = read_v11_bundle_path() {
        check_removed_injected_consumers(Some(&bundle), "v9");
    }
}

fn check_removed_injected_consumers(bundle: Option<&OsStr>, format: &str) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = registry_from_manifests(
        registry_dir.path(),
        [
            json!({"name":"p0", "version":"1.0.0", "dependencies":{"p2":"^2.0.0"}, "peerDependencies":{"p1":"*", "p2":"^2.0.0"}, "peerDependenciesMeta":{"p1":{"optional":true},"p2":{"optional":true}}}),
            json!({"name":"p0", "version":"2.0.0", "peerDependencies":{"p1":"*", "p2":"*"}, "peerDependenciesMeta":{"p1":{"optional":true},"p2":{"optional":true}}}),
            json!({"name":"p1", "version":"1.0.0"}),
            json!({"name":"p1", "version":"2.0.0", "peerDependencies":{"p0":"*", "p2":"^1.0.0"}}),
            json!({"name":"p2", "version":"1.0.0"}),
            json!({"name":"p2", "version":"2.0.0"}),
        ],
    );
    fixture.append_workspace_yaml(&format!("autoInstallPeers: true\nautoInstallPeersFromHighestMatch: false\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\ndedupePeerDependents: true\nlockfile:\n  format: {format}\n"));
    fixture.project("a", "app", ManifestDeps { prod: &[("p1", "^1.0.0")], ..Default::default() });
    fixture.project("b", "other", ManifestDeps { prod: &[("p0", "^2.0.0")], ..Default::default() });
    fixture.project(
        "shared",
        "shared",
        ManifestDeps { prod: &[("p2", "^2.0.0")], ..Default::default() },
    );
    let shared = fixture.workspace.join("packages/shared/package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&shared).unwrap()).unwrap();
    manifest["peerDependencies"] = json!({"p1":"*","p2":"^2.0.0"});
    manifest["peerDependenciesMeta"] = json!({"p1":{"optional":true},"p2":{"optional":true}});
    fs::write(shared, manifest.to_string()).unwrap();
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: &[("p0", "^1.0.0")], ..Default::default() },
    );
    for directory in [".", "packages/a", "packages/b"] {
        let file = fixture.workspace.join(directory).join("package.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        manifest["dependencies"]["shared"] = json!("workspace:*");
        manifest["dependenciesMeta"] = json!({"shared":{"injected":true}});
        fs::write(file, manifest.to_string()).unwrap();
    }
    fs::rename(fixture.workspace.join("packages/shared"), fixture.workspace.join("shared"))
        .unwrap();
    let yaml_path = fixture.workspace.join("pnpm-workspace.yaml");
    let mut yaml: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(&yaml_path).unwrap()).unwrap();
    yaml["packages"] = json!(["packages/*", "shared"]);
    fs::write(yaml_path, serde_saphyr::to_string(&yaml).unwrap()).unwrap();
    let args = &["install", "--lockfile-only", "--no-prefer-frozen-lockfile"];
    install(&fixture, bundle, args);
    for directory in [".", "packages/b"] {
        let file = fixture.workspace.join(directory).join("package.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        manifest["dependencies"]
            .as_object_mut()
            .unwrap()
            .remove("p0");
        fs::write(file, manifest.to_string()).unwrap();
        install(&fixture, bundle, args);
        let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        install(&fixture, bundle, args);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        dbg!(&changed, &reused);
        assert_eq!(changed, reused);
    }
}

#[test]
fn retained_cycle_provider_does_not_add_transitive_context() {
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
                json!({"name":"p0", "version":"1.0.0"}),
                json!({"name":"p0", "version":"2.0.0", "dependencies":{"p4":"^2.0.0"}, "peerDependencies":{"p4":"^2.0.0"}}),
                json!({"name":"p1", "version":"1.0.0", "dependencies":{"p4":"^2.0.0"}}),
                json!({"name":"p1", "version":"2.0.0", "dependencies":{"p0":"*"}, "peerDependencies":{"p0":"*"}}),
                json!({"name":"p4", "version":"1.0.0", "peerDependencies":{"p0":"^1.0.0"}}),
                json!({"name":"p4", "version":"2.0.0", "peerDependencies":{"p0":"*"}}),
            ],
        );
        fixture.append_workspace_yaml(&format!("autoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\ndedupePeerDependents: true\nlockfile:\n  format: {format}\n"));
        fixture.write_root_manifest(
            "root",
            ManifestDeps { prod: &[("p1", "^1.0.0"), ("p4", "^1.0.0")], ..Default::default() },
        );
        fixture.project(
            "a",
            "app",
            ManifestDeps { prod: &[("p0", "^2.0.0")], ..Default::default() },
        );
        let args = &["install", "--lockfile-only", "--no-prefer-frozen-lockfile"];
        install(&fixture, bundle, args);
        let fresh = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        install(&fixture, bundle, args);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        dbg!(&fresh, &reused);
        assert_eq!(fresh, reused);
    }
}

#[test]
fn cyclic_provider_keeps_its_optional_dependency_context() {
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
                json!({"name":"p0", "version":"1.0.0", "dependencies":{"p3":"^1.0.0"}, "peerDependencies":{"p3":"^1.0.0"}, "peerDependenciesMeta":{"p3":{"optional":true}}}),
                json!({"name":"p0", "version":"2.0.0", "peerDependencies":{"p2":"^1.0.0"}}),
                json!({"name":"p2", "version":"1.0.0", "dependencies":{"p0":"^1.0.0"}}),
                json!({"name":"p2", "version":"2.0.0", "dependencies":{"p0":"^1.0.0"}}),
                json!({"name":"p3", "version":"1.0.0", "peerDependencies":{"p2":"*"}}),
                json!({"name":"p3", "version":"2.0.0"}),
            ],
        );
        fixture.append_workspace_yaml(&format!("autoInstallPeers: true\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\ndedupePeerDependents: true\nlockfile:\n  format: {format}\n"));
        fixture.write_root_manifest(
            "root",
            ManifestDeps { prod: &[("p0", "^2.0.0")], ..Default::default() },
        );
        fixture.project(
            "a",
            "app",
            ManifestDeps { prod: &[("p3", "^1.0.0"), ("p2", "^2.0.0")], ..Default::default() },
        );
        let args = &["install", "--lockfile-only", "--no-prefer-frozen-lockfile"];
        install(&fixture, bundle, args);
        let fresh = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        install(&fixture, bundle, args);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        dbg!(&fresh, &reused);
        assert_eq!(fresh, reused, "{bundle:?}");
    }
}

#[test]
fn removing_a_direct_provider_keeps_its_consumers_context() {
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
                json!({"name":"p0", "version":"1.0.0", "dependencies":{"p1":"^2.0.0"}, "peerDependencies":{"p2":"^2.0.0"}}),
                json!({"name":"p0", "version":"2.0.0", "peerDependencies":{"p1":"^1.0.0"}}),
                json!({"name":"p1", "version":"1.0.0"}),
                json!({"name":"p1", "version":"2.0.0"}),
                json!({"name":"p2", "version":"1.0.0"}),
                json!({"name":"p2", "version":"2.0.0", "dependencies":{"p0":"^2.0.0"}, "peerDependencies":{"p1":"*"}}),
            ],
        );
        fixture.append_workspace_yaml(&format!("autoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\ndedupePeerDependents: true\nlockfile:\n  format: {format}\n"));
        fixture.write_root_manifest(
            "root",
            ManifestDeps { prod: &[("p2", "^2.0.0")], ..Default::default() },
        );
        fixture.project(
            "a",
            "app",
            ManifestDeps { prod: &[("p2", "^2.0.0")], ..Default::default() },
        );
        fixture.project(
            "b",
            "other",
            ManifestDeps { prod: &[("p1", "^1.0.0")], ..Default::default() },
        );
        let args = &["install", "--lockfile-only", "--no-prefer-frozen-lockfile"];
        install(&fixture, bundle, args);
        for (directory, alias, specifier) in [
            (".", "p1", Some("^1.0.0")),
            ("packages/a", "p0", Some("^1.0.0")),
            ("packages/a", "p2", None),
        ] {
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
            install(&fixture, bundle, args);
            let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            install(&fixture, bundle, args);
            let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
            dbg!(&changed, &reused);
            assert_eq!(changed, reused, "{bundle:?} {directory} {alias}");
        }
    }
}
