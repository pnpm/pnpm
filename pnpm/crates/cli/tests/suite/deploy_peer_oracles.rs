use crate::_utils::{
    ManifestDeps, TreeView, WorkspaceFixture, assert_success, read_manifest,
    resolve_node_modules_package, write_manifest_value,
};
use pnpm_lockfile::{BoundProviders, Lockfile, LockfileFormat};
use serde_json::json;
use std::{fs, path::Path};

#[derive(Debug, Clone, Copy)]
struct Scenario {
    optional_peer: bool,
    own_dependency: bool,
    catalogs: bool,
    short_suffix: bool,
    dedupe_peers: bool,
}

impl Scenario {
    fn all() -> impl Iterator<Item = Self> {
        (0..32).map(|bits| Self {
            optional_peer: bits & 1 != 0,
            own_dependency: bits & 2 != 0,
            catalogs: bits & 4 != 0,
            short_suffix: bits & 8 != 0,
            dedupe_peers: bits & 16 != 0,
        })
    }

    fn fixture(self, format: LockfileFormat, publish_directory: Option<&str>) -> WorkspaceFixture {
        let fixture = WorkspaceFixture::new();
        fixture.write_root_manifest("root", ManifestDeps::default());
        let format = match format {
            LockfileFormat::V9 => "v9",
            LockfileFormat::V13 => "v13",
        };
        // Hoisted siblings must not supply peers absent from this app's recorded graph.
        fixture.append_workspace_yaml(&format!(
            "hoistPattern: []\ninjectWorkspacePackages: true\ndedupeInjectedDeps: false\nautoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\ndedupePeerDependents: {}\npeersSuffixMaxLength: {}\nlockfile:\n  format: {format}\ncatalogs:\n  peers:\n    '@pnpm.e2e/foo': '^100.0.0'\n  children:\n    '@pnpm.e2e/foo': '100.0.0'\n",
            self.dedupe_peers,
            if self.short_suffix { 20 } else { 1000 },
        ));
        self.write_shared_package(&fixture, publish_directory);
        for (app, version) in
            [("app-a", Some("100.0.0")), ("app-b", Some("100.1.0")), ("app-c", None)]
        {
            let mut dependencies = vec![("shared", "workspace:*")];
            if let Some(version) = version {
                dependencies.push(("@pnpm.e2e/foo", version));
            }
            fixture.project(app, app, ManifestDeps { prod: &dependencies, ..Default::default() });
        }
        fixture
    }

    fn write_shared_package(self, fixture: &WorkspaceFixture, publish_directory: Option<&str>) {
        let shared = fixture.project("shared", "shared", ManifestDeps::default());
        let mut manifest = json!({
            "name": "shared", "version": "1.0.0",
            "peerDependencies": {
                "@pnpm.e2e/foo": if self.catalogs { "catalog:peers" } else { "^100.0.0" },
            },
            "peerDependenciesMeta": { "@pnpm.e2e/foo": { "optional": self.optional_peer } },
        });
        if self.own_dependency {
            manifest["dependencies"] = json!({
                "@pnpm.e2e/foo": if self.catalogs { "catalog:children" } else { "100.0.0" },
            });
        }
        if let Some(directory) = publish_directory {
            fs::create_dir_all(shared.join(directory)).unwrap();
            write_manifest_value(&shared.join(directory), &manifest);
            manifest["publishConfig"] = json!({ "directory": directory });
        }
        write_manifest_value(&shared, &manifest);
    }
}

#[test]
fn frozen_check_accepts_v9_deploy_output() {
    check_deploys(LockfileFormat::V9, None);
}

#[test]
fn frozen_check_accepts_v13_deploy_output() {
    check_deploys(LockfileFormat::V13, None);
}

#[test]
fn frozen_check_accepts_v9_publish_directory_deploy_output() {
    check_deploys(LockfileFormat::V9, Some("dist"));
}

#[test]
fn frozen_check_accepts_v13_publish_directory_deploy_output() {
    check_deploys(LockfileFormat::V13, Some("dist"));
}

#[test]
fn deploy_retains_hashed_bindings_when_relocating_workspace_snapshots() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest("root", ManifestDeps::default());
    fixture.append_workspace_yaml(
        "injectWorkspacePackages: true\ndedupeInjectedDeps: false\nautoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\npeersSuffixMaxLength: 3\nlockfile:\n  format: v13\n",
    );
    let shared = fixture.project("shared", "shared", ManifestDeps::default());
    let mut manifest = json!({ "name": "shared", "version": "1.0.0" });
    for index in 0..14 {
        let name = format!("peer-{index:02}");
        manifest["dependencies"][&name] = json!("npm:@pnpm.e2e/foo@100.0.0");
        manifest["peerDependencies"][&name] = json!("*");
    }
    write_manifest_value(&shared, &manifest);
    fixture.project(
        "app",
        "app",
        ManifestDeps {
            prod: &[("shared", "workspace:*"), ("peer-00", "npm:@pnpm.e2e/foo@100.1.0")],
            ..Default::default()
        },
    );
    fixture.run(["install"]);
    let lockfile = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    let (shared_key, _) = lockfile.snapshots
        .as_ref()
        .unwrap()
        .iter()
        .find(|(key, _)| {
            key.name.to_string() == "shared" && BoundProviders::from_key(key).is_hashed()
        })
        .expect("a hashed shared snapshot");
    assert!(!BoundProviders::of_snapshot((&lockfile).into(), shared_key).is_hashed());
    let source_lockfile = fs::read(fixture.workspace.join("pnpm-lock.yaml")).unwrap();
    let source = fixture.workspace.join("packages/app");
    let deployed = fixture.workspace.with_file_name("deploy-retained-bindings");
    assert_success(&fixture.command_at(
        &fixture.workspace,
        ["--filter", "app", "deploy", &deployed.to_string_lossy()],
    ));
    assert_eq!(
        TreeView::new(&deployed).project(&deployed, false),
        TreeView::new(&fixture.workspace).project(&source, false),
    );
    check_frozen_deploy(&fixture, &deployed, None);
    assert_eq!(fs::read(fixture.workspace.join("pnpm-lock.yaml")).unwrap(), source_lockfile);
}

#[test]
fn deploy_retains_hashed_workspace_peer_bindings() {
    for workspace_name in ["workspace", "workspace (with spaces) café"] {
        check_workspace_peer_bindings(workspace_name);
    }
}

fn check_workspace_peer_bindings(workspace_name: &str) {
    let provider = "provider";
    let mut fixture = WorkspaceFixture::new();
    let workspace = fixture.workspace.with_file_name(workspace_name);
    if workspace != fixture.workspace {
        fs::rename(&fixture.workspace, &workspace).unwrap();
        fixture.workspace = workspace;
    }
    fixture.write_root_manifest("root", ManifestDeps::default());
    fixture.append_workspace_yaml(
        "injectWorkspacePackages: true\ndedupeInjectedDeps: false\nautoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\npeersSuffixMaxLength: 3\nlockfile:\n  format: v13\n",
    );
    let dependencies = vec![("shared", "workspace:*"), (provider, "workspace:*")];
    fixture.project(provider, provider, ManifestDeps::default());
    let shared = fixture.project("shared", "shared", ManifestDeps::default());
    write_manifest_value(
        &shared,
        &json!({
            "name": "shared", "version": "1.0.0", "peerDependencies": {provider: "*"},
        }),
    );
    fixture.project("app", "app", ManifestDeps { prod: &dependencies, ..Default::default() });
    fixture.run(["install"]);
    let deployed = fixture.workspace.with_file_name("deploy-workspace-provider");
    assert_success(&fixture.command_at(
        &fixture.workspace,
        ["--filter", "app", "deploy", &deployed.to_string_lossy()],
    ));
    assert_eq!(
        TreeView::new(&deployed).project(&deployed, false),
        TreeView::new(&fixture.workspace).project(&fixture.workspace.join("packages/app"), false),
    );
    let lockfile = Lockfile::load_wanted_from_dir(&deployed).unwrap().unwrap();
    let sections = lockfile.to_next_sections().unwrap();
    let (_, entries) = sections.snapshots
        .as_ref()
        .unwrap()
        .iter()
        .find(|(key, _)| key.name.to_string() == "shared")
        .unwrap();
    assert!(
        entries
            .iter()
            .all(|entry| entry.peer_dependencies
                .as_ref()
                .is_some_and(|peers| peers.contains_key(&provider.parse().unwrap()))),
        "{provider}: {entries:?}",
    );
    check_frozen_deploy(&fixture, &deployed, None);
}

#[test]
fn deploy_keeps_links_to_the_deployed_project() {
    for format in ["v9", "v13"] {
        check_deployed_self_link(format);
    }
}

fn check_deployed_self_link(format: &str) {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest("root", ManifestDeps::default());
    fixture.append_workspace_yaml(&format!(
        "injectWorkspacePackages: true\nlockfile:\n  format: {format}\n",
    ));
    fixture.project(
        "app",
        "app",
        ManifestDeps { prod: &[("app", "workspace:*")], ..Default::default() },
    );
    fixture.run(["install"]);
    let deployed = fixture.workspace.with_file_name("deploy-self-link");
    assert_success(&fixture.command_at(
        &fixture.workspace,
        ["--filter", "app", "deploy", &deployed.to_string_lossy()],
    ));
    let linked = resolve_node_modules_package(&deployed, "app").unwrap();
    assert_eq!(fs::canonicalize(linked).unwrap(), fs::canonicalize(&deployed).unwrap());
    let lockfile = fs::read(deployed.join("pnpm-lock.yaml")).unwrap();
    fs::copy(fixture.workspace.join(".npmrc"), deployed.join(".npmrc")).unwrap();
    fs::remove_dir_all(deployed.join("node_modules")).unwrap();
    assert_success(&fixture.command_at(&deployed, ["install", "--frozen-lockfile"]));
    assert_eq!(fs::read(deployed.join("pnpm-lock.yaml")).unwrap(), lockfile);
}

fn check_deploys(format: LockfileFormat, publish_directory: Option<&str>) {
    for scenario in Scenario::all() {
        eprintln!("deploy oracle: {format:?} {scenario:?}");
        let fixture = scenario.fixture(format, publish_directory);
        fixture.run(["install"]);
        let source = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        assert_eq!(source.lockfile_version, format.version());
        if scenario.short_suffix {
            assert!(
                source.snapshots
                    .as_ref()
                    .unwrap()
                    .keys()
                    .any(|key| { BoundProviders::from_key(key).is_hashed() }),
                "the short suffix case must exercise hashed peer bindings",
            );
        }
        let source_lockfile = fs::read(fixture.workspace.join("pnpm-lock.yaml")).unwrap();
        for app in ["app-a", "app-b", "app-c"] {
            let source = fixture.workspace.join("packages").join(app);
            let expected_peer = shared_peer_version(&source);

            let deployed = fixture.workspace.with_file_name(format!("deploy-{app}"));
            let output = fixture.command_at(
                &fixture.workspace,
                ["--filter", app, "deploy", &deployed.to_string_lossy()],
            );
            assert_success(&output);
            assert_eq!(
                Lockfile::load_wanted_from_dir(&deployed).unwrap().unwrap().lockfile_version,
                format.version(),
            );
            assert_eq!(
                TreeView::new(&deployed).project(&deployed, false),
                TreeView::new(&fixture.workspace).project(&source, false),
                "{app}",
            );
            check_frozen_deploy(&fixture, &deployed, expected_peer.as_deref());
        }
        assert_eq!(fs::read(fixture.workspace.join("pnpm-lock.yaml")).unwrap(), source_lockfile);
    }
}

fn check_frozen_deploy(fixture: &WorkspaceFixture, deployed: &Path, expected_peer: Option<&str>) {
    assert_eq!(shared_peer_version(deployed).as_deref(), expected_peer, "{}", deployed.display());
    let expected_lockfile = fs::read(deployed.join("pnpm-lock.yaml")).unwrap();
    let expected_tree = TreeView::new(deployed).project(deployed, false);
    assert!(expected_tree.contains("shared@1.0.0"), "{expected_tree}");
    fs::copy(fixture.workspace.join(".npmrc"), deployed.join(".npmrc")).unwrap();
    fs::remove_dir_all(deployed.join("node_modules")).unwrap();
    assert_success(&fixture.command_at(deployed, ["install", "--frozen-lockfile"]));
    assert_eq!(fs::read(deployed.join("pnpm-lock.yaml")).unwrap(), expected_lockfile);
    assert_eq!(TreeView::new(deployed).project(deployed, false), expected_tree);
    assert_eq!(shared_peer_version(deployed).as_deref(), expected_peer, "{}", deployed.display());
    let mut stale = read_manifest(deployed);
    stale["dependencies"]["shared"] = json!("0.0.0");
    write_manifest_value(deployed, &stale);
    let rejected = fixture.command_at(deployed, ["install", "--frozen-lockfile"]);
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&rejected.stdout),
        String::from_utf8_lossy(&rejected.stderr),
    );
    assert!(
        !rejected.status.success(),
        "a changed manifest must not pass the frozen check: {output}",
    );
    assert!(output.contains("ERR_PNPM_OUTDATED_LOCKFILE"), "{output}");
    assert_eq!(fs::read(deployed.join("pnpm-lock.yaml")).unwrap(), expected_lockfile);
}

fn shared_peer_version(project: &Path) -> Option<String> {
    let shared = resolve_node_modules_package(project, "shared").expect("installed shared package");
    let shared = fs::canonicalize(shared).unwrap();
    resolve_node_modules_package(&shared, "@pnpm.e2e/foo")
        .map(|provider| {
            read_manifest(&provider)["version"]
                .as_str()
                .unwrap()
                .to_string()
        })
}
