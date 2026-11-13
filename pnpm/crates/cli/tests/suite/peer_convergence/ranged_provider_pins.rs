use super::moving_providers::{install_lockfile, registry_from_manifests};
use crate::_utils::{ManifestDeps, WorkspaceFixture, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use serde_json::json;
use std::{ffi::OsStr, fs};

#[derive(Debug, Clone, Copy)]
enum InitialProvider {
    Declared,
    Added,
    OlderPin,
}

#[test]
fn injected_peer_only_importer_keeps_its_preferred_version() {
    for auto_dedupe in [false, true] {
        for initial in
            [InitialProvider::Declared, InitialProvider::Added, InitialProvider::OlderPin]
        {
            for format in ["v9", "v13"] {
                check_repeated_installs(None, format, auto_dedupe, initial);
            }
            if let Some(bundle) = read_v11_bundle_path() {
                check_repeated_installs(Some(&bundle), "v9", auto_dedupe, initial);
            }
        }
    }
}

fn check_repeated_installs(
    bundle: Option<&OsStr>,
    format: &str,
    auto_dedupe: bool,
    initial: InitialProvider,
) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = registry_from_manifests(
        registry_dir.path(),
        [
            json!({ "name": "provider", "version": "1.0.0" }),
            json!({ "name": "provider", "version": "2.0.0" }),
            json!({ "name": "provider", "version": "2.1.0" }),
        ],
    );
    fixture.append_workspace_yaml(&format!(
        "autoInstallPeers: true\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\nautoDedupe: {auto_dedupe}\nlockfile:\n  format: {format}\n",
    ));
    fixture.project("a", "app", ManifestDeps::default());
    let shared = fixture.project("shared", "shared", ManifestDeps::default());
    fs::write(
        shared.join("package.json"),
        json!({
            "name": "shared", "version": "1.0.0", "peerDependencies": { "provider": "*" },
        })
        .to_string(),
    )
    .unwrap();
    write_consumer(&fixture, ".", "^2.0.0");
    write_consumer(&fixture, "packages/a", "^1.0.0");
    if matches!(initial, InitialProvider::OlderPin) {
        write_consumer(&fixture, ".", "2.0.0");
        install_lockfile(&fixture, bundle);
        write_consumer(&fixture, ".", "^2.0.0");
        // Emulate a lockfile created before the newer minor version was published.
        let mut lockfile = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        lockfile.importers
            .get_mut(".")
            .unwrap()
            .dependencies
            .as_mut()
            .unwrap()
            .get_mut(&"provider".parse().unwrap())
            .unwrap()
            .specifier = "^2.0.0".into();
        fs::write(fixture.workspace.join("pnpm-lock.yaml"), lockfile.to_yaml_string().unwrap())
            .unwrap();
    }
    if matches!(initial, InitialProvider::Added) {
        let path = fixture.workspace.join("package.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        manifest["dependencies"]
            .as_object_mut()
            .unwrap()
            .remove("provider");
        fs::write(path, manifest.to_string()).unwrap();
        install_lockfile(&fixture, bundle);
        write_consumer(&fixture, ".", "^2.0.0");
    }
    install_lockfile(&fixture, bundle);
    let fresh = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    if matches!(initial, InitialProvider::OlderPin) {
        assert_eq!(
            fresh.importers["packages/shared"].dependencies.as_ref().unwrap()
                [&"provider".parse().unwrap()]
                .version
                .to_string(),
            "2.0.0",
            "{bundle:?} {format}",
        );
    }
    for _ in 0..2 {
        install_lockfile(&fixture, bundle);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        dbg!(&fresh, &reused);
        assert_eq!(
            fresh, reused,
            "{bundle:?}, {format}, autoDedupe={auto_dedupe}, initial={initial:?}",
        );
    }
}

fn write_consumer(fixture: &WorkspaceFixture, directory: &str, specifier: &str) {
    fs::write(
        fixture.workspace.join(directory).join("package.json"),
        json!({
            "name": if directory == "." { "root" } else { "app" },
            "version": "1.0.0",
            "dependencies": { "provider": specifier, "shared": "workspace:*" },
            "dependenciesMeta": { "shared": { "injected": true } },
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn adding_a_cyclic_provider_keeps_injected_peers_stable() {
    if let Some(bundle) = read_v11_bundle_path() {
        check_cyclic_provider(Some(&bundle), "v9");
    }
    for format in ["v9", "v13"] {
        check_cyclic_provider(None, format);
    }
}

fn check_cyclic_provider(bundle: Option<&OsStr>, format: &str) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = registry_from_manifests(
        registry_dir.path(),
        [
            json!({"name":"p0", "version":"1.0.0", "peerDependencies":{"p1":"*"}}),
            json!({"name":"p0", "version":"2.0.0"}),
            json!({"name":"p1", "version":"1.0.0", "peerDependencies":{"p0":"^1.0.0"}}),
            json!({"name":"p1", "version":"2.0.0"}),
        ],
    );
    fixture.append_workspace_yaml(&format!("autoInstallPeers: true\nresolvePeersFromWorkspaceRoot: false\ndedupePeers: false\ndedupeInjectedDeps: false\nautoDedupe: false\nlockfile:\n  format: {format}\n"));
    fixture.project("a", "app", ManifestDeps::default());
    let shared = fixture.project("shared", "shared", ManifestDeps::default());
    fs::write(
        shared.join("package.json"),
        json!({"name":"shared", "version":"1.0.0", "peerDependencies":{"p1":"*"}}).to_string(),
    )
    .unwrap();
    fs::write(fixture.workspace.join("packages/a/package.json"), json!({"name":"app", "version":"1.0.0", "dependencies":{"p1":"^1.0.0", "shared":"workspace:*"}, "dependenciesMeta":{"shared":{"injected":true}}}).to_string()).unwrap();
    let mut root = json!({"name":"root", "version":"1.0.0", "dependencies":{"p0":"^1.0.0", "shared":"workspace:*"}, "dependenciesMeta":{"shared":{"injected":true}}});
    let path = fixture.workspace.join("package.json");
    fs::write(&path, root.to_string()).unwrap();
    install_lockfile(&fixture, bundle);
    root["dependencies"]["p1"] = json!("^2.0.0");
    fs::write(&path, root.to_string()).unwrap();
    install_lockfile(&fixture, bundle);
    let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    install_lockfile(&fixture, bundle);
    let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    dbg!(&changed, &reused);
    assert_eq!(changed, reused, "{bundle:?} {format}");
}
