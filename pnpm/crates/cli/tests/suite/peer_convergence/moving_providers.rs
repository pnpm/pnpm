use crate::_utils::{ManifestDeps, WorkspaceFixture, assert_success, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::registry::TestRegistry;
use serde_json::{Value, json};
use std::{ffi::OsStr, fs, path::Path};

#[test]
fn changing_a_consumer_keeps_optional_peer_pins_on_cycles() {
    for suffix_length in [20, 1000] {
        for format in ["v9", "v13"] {
            check_provider_pin(None, format, suffix_length);
        }
        if let Some(bundle) = read_v11_bundle_path() {
            check_provider_pin(Some(&bundle), "v9", suffix_length);
        }
    }
}

fn check_provider_pin(bundle: Option<&OsStr>, format: &str, suffix_length: usize) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = cycle_registry(registry_dir.path());
    fixture.append_workspace_yaml(&format!(
        "autoInstallPeers: true\nresolvePeersFromWorkspaceRoot: false\ndedupePeerDependents: true\npeersSuffixMaxLength: {suffix_length}\nlockfile:\n  format: {format}\n",
    ));
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: &[("cycle-provider", "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "a",
        "app",
        ManifestDeps { prod: &[("cycle-consumer", "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "b",
        "other",
        ManifestDeps { prod: &[("cycle-provider", "2.0.0")], ..Default::default() },
    );
    let install = || install_lockfile(&fixture, bundle);
    install();
    fixture.project(
        "a",
        "app",
        ManifestDeps { prod: &[("cycle-consumer", "2.0.0")], ..Default::default() },
    );
    install();
    let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    assert_updated_consumer_provider(&changed);
    install();
    let reinstalled = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    dbg!(&changed, &reinstalled);
    assert_eq!(changed, reinstalled, "{bundle:?}, {format}, {suffix_length}");
}

fn assert_updated_consumer_provider(lockfile: &Lockfile) {
    let consumer_name = "cycle-consumer".parse().unwrap();
    let bridge_name = "cycle-bridge".parse().unwrap();
    let provider_name = "cycle-provider".parse().unwrap();
    let consumer = lockfile.importers["packages/a"].dependencies.as_ref().unwrap()[&consumer_name]
        .version
        .resolved_key(&consumer_name)
        .unwrap();
    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let bridge = snapshots[&consumer].dependencies.as_ref().unwrap()[&bridge_name]
        .resolve(&bridge_name)
        .unwrap();
    let provider = snapshots[&bridge].optional_dependencies.as_ref().unwrap()[&provider_name]
        .resolve(&provider_name)
        .unwrap();
    assert_eq!(provider.without_peer().to_string(), "cycle-provider@2.0.0");
}

fn cycle_registry(root: &Path) -> TestRegistry {
    let manifests = [
        ("cycle-consumer", "1.0.0", json!({ "cycle-provider": "^1.0.0" }), json!({})),
        ("cycle-consumer", "2.0.0", json!({ "cycle-bridge": "*" }), json!({})),
        (
            "cycle-bridge",
            "2.0.0",
            json!({ "cycle-provider": "*" }),
            json!({ "cycle-provider": { "optional": true } }),
        ),
        ("cycle-provider", "1.0.0", json!({ "cycle-bridge": "^2.0.0" }), json!({})),
        ("cycle-provider", "2.0.0", json!({}), json!({})),
    ]
    .map(|(name, version, peers, optional)| {
        json!({
            "name": name,
            "version": version,
            "peerDependencies": peers,
            "peerDependenciesMeta": optional,
        })
    });
    registry_from_manifests(root, manifests)
}

fn registry_from_manifests(
    root: &Path,
    manifests: impl IntoIterator<Item = Value>,
) -> TestRegistry {
    let packages = root.join("packages");
    for manifest in manifests {
        let directory = packages
            .join(manifest["name"].as_str().unwrap())
            .join(manifest["version"].as_str().unwrap());
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("package.json"), manifest.to_string()).unwrap();
    }
    let storage = root.join("storage");
    pnpr_fixtures::build_storage_at(&packages, &storage);
    TestRegistry::start_static_with_storage(&storage)
}

fn install_lockfile(fixture: &WorkspaceFixture, bundle: Option<&OsStr>) {
    let args = ["install", "--lockfile-only", "--no-prefer-frozen-lockfile"];
    let output = match bundle {
        Some(bundle) => fixture.v11_command_at(bundle, &fixture.workspace, args),
        None => fixture.command_at(&fixture.workspace, args),
    };
    assert_success(&output);
}

#[test]
fn changing_a_root_provider_keeps_nested_peer_edges() {
    for format in ["v9", "v13"] {
        check_required_cycle(None, format);
    }
    if let Some(bundle) = read_v11_bundle_path() {
        check_required_cycle(Some(&bundle), "v9");
    }
}

fn check_required_cycle(bundle: Option<&OsStr>, format: &str) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = registry_from_manifests(
        registry_dir.path(),
        [
            json!({ "name": "cycle-source", "version": "1.0.0", "dependencies": { "cycle-middle": "1.0.0" } }),
            json!({ "name": "cycle-source", "version": "2.0.0" }),
            json!({ "name": "cycle-middle", "version": "1.0.0", "dependencies": { "cycle-leaf": "2.0.0" }, "peerDependencies": { "cycle-leaf": "^2.0.0" } }),
            json!({ "name": "cycle-middle", "version": "2.0.0", "dependencies": { "cycle-source": "1.0.0" }, "peerDependencies": { "cycle-source": "^1.0.0" } }),
            json!({ "name": "cycle-leaf", "version": "2.0.0", "dependencies": { "cycle-middle": "2.0.0", "cycle-source": "2.0.0" } }),
        ],
    );
    fixture.append_workspace_yaml(&format!("autoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\ndedupeInjectedDeps: false\npeersSuffixMaxLength: 20\nlockfile:\n  format: {format}\n"));
    fixture.project(
        "shared",
        "shared",
        ManifestDeps { prod: &[("cycle-middle", "1.0.0")], ..Default::default() },
    );
    let root_manifest = |version| json!({ "name": "root", "version": "1.0.0", "dependencies": { "cycle-source": version, "shared": "workspace:*" }, "dependenciesMeta": { "shared": { "injected": true } } });
    fs::write(fixture.workspace.join("package.json"), root_manifest("2.0.0").to_string()).unwrap();
    install_lockfile(&fixture, bundle);
    fs::write(fixture.workspace.join("package.json"), root_manifest("1.0.0").to_string()).unwrap();
    install_lockfile(&fixture, bundle);
    let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    install_lockfile(&fixture, bundle);
    let reinstalled = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    assert_eq!(nested_provider(&changed), "cycle-source@1.0.0", "{bundle:?}, {format}");
    assert_eq!(nested_provider(&changed), nested_provider(&reinstalled), "{bundle:?}, {format}");
}

fn nested_provider(lockfile: &Lockfile) -> String {
    let shared = "shared".parse().unwrap();
    let mut key = lockfile.importers["."].dependencies.as_ref().unwrap()[&shared]
        .version
        .resolved_key(&shared)
        .unwrap();
    for name in ["cycle-middle", "cycle-leaf", "cycle-middle", "cycle-source"] {
        let name = name.parse().unwrap();
        key = lockfile.snapshots.as_ref().unwrap()[&key].dependencies.as_ref().unwrap()[&name]
            .resolve(&name)
            .unwrap();
    }
    key.without_peer().to_string()
}

#[test]
fn removing_a_direct_provider_keeps_auto_installed_peers_stable() {
    if let Some(bundle) = read_v11_bundle_path() {
        check_removed_provider(Some(&bundle), "v9");
    }
    for format in ["v9", "v13"] {
        check_removed_provider(None, format);
    }
}

fn check_removed_provider(bundle: Option<&OsStr>, format: &str) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = registry_from_manifests(
        registry_dir.path(),
        [
            json!({"name": "removed-provider", "version": "1.0.0"}),
            json!({"name": "removed-provider", "version": "2.0.0"}),
            json!({"name": "retained-consumer", "version": "2.0.0", "peerDependencies": {"removed-provider": "*"}}),
        ],
    );
    fixture.append_workspace_yaml(&format!(
        "autoInstallPeers: true\nlockfile:\n  format: {format}\n",
    ));
    fixture.write_root_manifest(
        "root",
        ManifestDeps {
            prod: &[("removed-provider", "1.0.0"), ("retained-consumer", "2.0.0")],
            ..Default::default()
        },
    );
    install_lockfile(&fixture, bundle);
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: &[("retained-consumer", "2.0.0")], ..Default::default() },
    );
    install_lockfile(&fixture, bundle);
    let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    install_lockfile(&fixture, bundle);
    let reinstalled = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    dbg!(&changed, &reinstalled);
    assert_eq!(changed, reinstalled, "{bundle:?}, {format}");
}

#[test]
fn removing_nested_optional_providers_keeps_peer_bindings_stable() {
    if let Some(bundle) = read_v11_bundle_path() {
        check_nested_removal(Some(&bundle), "v9");
    }
    for format in ["v9", "v13"] {
        check_nested_removal(None, format);
    }
}

fn check_nested_removal(bundle: Option<&OsStr>, format: &str) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = registry_from_manifests(
        registry_dir.path(),
        [
            json!({"name": "nested-parent", "version": "1.0.0"}),
            json!({"name": "nested-parent", "version": "2.0.0", "dependencies": {"nested-child": "^2.0.0"}}),
            json!({"name": "nested-provider", "version": "1.0.0", "peerDependencies": {"nested-child": "^1.0.0"}}),
            json!({"name": "nested-provider", "version": "2.0.0", "peerDependencies": {"nested-parent": "^2.0.0"}}),
            json!({"name": "nested-child", "version": "1.0.0"}),
            json!({"name": "nested-child", "version": "2.0.0", "peerDependencies": {"nested-provider": "^1.0.0"}, "peerDependenciesMeta": {"nested-provider": {"optional": true}}}),
        ],
    );
    fixture.append_workspace_yaml(&format!("autoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\ndedupeInjectedDeps: false\npeersSuffixMaxLength: 20\nlockfile:\n  format: {format}\n"));
    fixture.project("shared", "shared", ManifestDeps::default());
    let manifest = |name, deps: Value| {
        let mut manifest = json!({"name": name, "version": "1.0.0", "dependencies": deps, "dependenciesMeta": {"shared": {"injected": true}}});
        manifest["dependencies"]["shared"] = json!("workspace:*");
        manifest
    };
    fs::write(
        fixture.workspace.join("package.json"),
        manifest("root", json!({"nested-provider": "^2.0.0"})).to_string(),
    )
    .unwrap();
    fixture.project("a", "app", ManifestDeps::default());
    fixture.project("b", "other", ManifestDeps::default());
    fs::write(
        fixture.workspace.join("packages/b/package.json"),
        manifest("other", json!({"nested-child": "^2.0.0"})).to_string(),
    )
    .unwrap();
    let steps = [
        json!({"nested-parent": "^2.0.0"}),
        json!({"nested-parent": "^2.0.0", "nested-provider": "^1.0.0"}),
        json!({"nested-provider": "^1.0.0"}),
        json!({}),
    ];
    for dependencies in steps {
        fs::write(
            fixture.workspace.join("packages/a/package.json"),
            manifest("app", dependencies.clone()).to_string(),
        )
        .unwrap();
        install_lockfile(&fixture, bundle);
        let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        install_lockfile(&fixture, bundle);
        let reinstalled = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        dbg!(&changed, &reinstalled);
        assert_eq!(changed, reinstalled, "{bundle:?}, {format}, {dependencies}");
    }
}

#[test]
fn adding_a_provider_updates_optional_candidates_children() {
    if let Some(bundle) = read_v11_bundle_path() {
        check_added_provider(Some(&bundle), "v9");
    }
    for format in ["v9", "v13"] {
        check_added_provider(None, format);
    }
}

fn check_added_provider(bundle: Option<&OsStr>, format: &str) {
    let registry_dir = tempfile::tempdir().unwrap();
    let mut fixture = WorkspaceFixture::new();
    fixture.registry.mock_instance = registry_from_manifests(
        registry_dir.path(),
        [
            json!({"name": "optional-candidate", "version": "1.0.0"}),
            json!({"name": "optional-candidate", "version": "2.0.0", "dependencies": {"added-provider": "^2.0.0"}, "peerDependencies": {"added-provider": "^2.0.0"}}),
            json!({"name": "added-provider", "version": "1.0.0"}),
            json!({"name": "added-provider", "version": "2.0.0", "peerDependencies": {"optional-candidate": "^1.0.0"}}),
            json!({"name": "optional-consumer", "version": "1.0.0", "peerDependencies": {"optional-candidate": "*"}, "peerDependenciesMeta": {"optional-candidate": {"optional": true}}}),
        ],
    );
    fixture.append_workspace_yaml(&format!("autoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\nlockfile:\n  format: {format}\n"));
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: &[("optional-consumer", "^1.0.0")], ..Default::default() },
    );
    fixture.project(
        "a",
        "app",
        ManifestDeps { prod: &[("added-provider", "^1.0.0")], ..Default::default() },
    );
    install_lockfile(&fixture, bundle);
    fixture.project(
        "a",
        "app",
        ManifestDeps {
            prod: &[("added-provider", "^1.0.0"), ("optional-candidate", "^2.0.0")],
            ..Default::default()
        },
    );
    install_lockfile(&fixture, bundle);
    let first = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    install_lockfile(&fixture, bundle);
    let repeated = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    dbg!(&first, &repeated);
    assert_eq!(first, repeated, "first change: {bundle:?}, {format}");
    assert_optional_candidate_child(&first);
    fixture.write_root_manifest(
        "root",
        ManifestDeps {
            prod: &[("optional-consumer", "^1.0.0"), ("added-provider", "^1.0.0")],
            ..Default::default()
        },
    );
    install_lockfile(&fixture, bundle);
    let changed = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    install_lockfile(&fixture, bundle);
    let reinstalled = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    dbg!(&changed, &reinstalled);
    assert_eq!(changed, reinstalled, "{bundle:?}, {format}");
}

fn assert_optional_candidate_child(lockfile: &Lockfile) {
    let consumer_name = "optional-consumer".parse().unwrap();
    let candidate_name = "optional-candidate".parse().unwrap();
    let provider_name = "added-provider".parse().unwrap();
    let consumer = lockfile.importers["."].dependencies.as_ref().unwrap()[&consumer_name]
        .version
        .resolved_key(&consumer_name)
        .unwrap();
    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let candidate = snapshots[&consumer].optional_dependencies.as_ref().unwrap()[&candidate_name]
        .resolve(&candidate_name)
        .unwrap();
    assert_eq!(candidate.without_peer().to_string(), "optional-candidate@2.0.0");
    let provider = snapshots[&candidate].dependencies.as_ref().unwrap()[&provider_name]
        .resolve(&provider_name)
        .unwrap();
    assert_eq!(provider.without_peer().to_string(), "added-provider@2.0.0");
}
