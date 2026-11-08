//! `addMissingPeerTypes` links the `@types` package of a peer dependency next to
//! the peer dependent, which the global virtual store needs for TypeScript to
//! find it.
//!
//! `@pnpm.e2e/abc-optional-peers` peer-depends on `@pnpm.e2e/peer-a`, whose
//! types package is `@types/pnpm.e2e__peer-a`.

use crate::_utils::{ManifestDeps, WorkspaceFixture};

use std::{
    fs,
    path::{Path, PathBuf},
};

const ABC: &str = "@pnpm.e2e/abc-optional-peers";
const TYPES_PEER_A: &str = "@types/pnpm.e2e__peer-a";

fn fixture(settings: &str) -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    let yaml_path = fixture.workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path)
        .expect("read pnpm-workspace.yaml")
        .replace("enableGlobalVirtualStore: false\n", "enableGlobalVirtualStore: true\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    fixture.append_workspace_yaml(settings);
    fixture.write_root_manifest(
        "root",
        ManifestDeps {
            prod: &[(ABC, "1.0.0"), ("@pnpm.e2e/peer-a", "1.0.0"), (TYPES_PEER_A, "1.0.0")],
            ..ManifestDeps::default()
        },
    );
    fixture
}

/// The directory `node_modules/<ABC>` resolves into, holding the package and
/// the dependencies linked next to it.
fn abc_slot(workspace: &Path) -> PathBuf {
    fs::canonicalize(workspace.join("node_modules").join(ABC))
        .expect("resolve the peer dependent's link")
        .ancestors()
        .nth(3)
        .expect("a scoped package inside a slot has a slot")
        .to_path_buf()
}

fn links_types_next_to_abc(fixture: &WorkspaceFixture) -> bool {
    abc_slot(&fixture.workspace)
        .join("node_modules")
        .join(TYPES_PEER_A)
        .join("package.json")
        .is_file()
}

fn wanted_lockfile_text(fixture: &WorkspaceFixture) -> String {
    fs::read_to_string(fixture.workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml")
}

#[test]
fn links_the_types_package_next_to_the_peer_dependent() {
    let fixture = fixture("addMissingPeerTypes: true\n");

    fixture.run(["install"]);

    assert!(links_types_next_to_abc(&fixture));
    assert_eq!(
        fixture.wanted().settings.and_then(|settings| settings.add_missing_peer_types),
        Some(true),
    );
    let lockfile = wanted_lockfile_text(&fixture);
    eprintln!("{lockfile}");
    assert!(lockfile.contains(&format!("({TYPES_PEER_A}@1.0.0)")));
}

#[test]
fn links_no_undeclared_types_package_by_default() {
    let fixture = fixture("");

    fixture.run(["install"]);

    assert!(!links_types_next_to_abc(&fixture));
    let lockfile = wanted_lockfile_text(&fixture);
    eprintln!("{lockfile}");
    assert!(!lockfile.contains("addMissingPeerTypes"));
    assert!(!lockfile.contains(&format!("({TYPES_PEER_A}@1.0.0)")));
}

#[test]
fn turning_the_setting_on_re_resolves_an_existing_lockfile() {
    let fixture = fixture("");
    fixture.run(["install"]);
    fixture.append_workspace_yaml("addMissingPeerTypes: true\n");

    fixture.run(["install"]);

    assert!(links_types_next_to_abc(&fixture));
}

#[test]
fn a_frozen_install_rejects_a_lockfile_resolved_without_the_setting() {
    let fixture = fixture("");
    fixture.run(["install"]);
    fixture.append_workspace_yaml("addMissingPeerTypes: true\n");

    let output = fixture.command_at(&fixture.workspace, ["install", "--frozen-lockfile"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(!output.status.success());
    assert!(format!("{stdout}{stderr}").contains("addMissingPeerTypes"));
}

#[test]
fn injected_workspace_peers_pass_frozen_install() {
    for (shared_lockfile, hook_action) in [
        (true, "none"),
        (false, "none"),
        (true, "remove"),
        (false, "remove"),
        (true, "add"),
        (false, "add"),
    ] {
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml("injectWorkspacePackages: true\naddMissingPeerTypes: true\n");
        if !shared_lockfile {
            fixture.append_workspace_yaml("sharedWorkspaceLockfile: false\n");
        }
        fixture.write_root_manifest(
            "root",
            ManifestDeps { prod: &[(TYPES_PEER_A, "1.0.0")], ..ManifestDeps::default() },
        );
        if hook_action == "remove" {
            fs::write(
                fixture.workspace.join(".pnpmfile.cjs"),
                r"
module.exports = { hooks: { readPackage(pkg) {
  if (pkg.name === 'library') {
    delete pkg.peerDependencies['@types/pnpm.e2e__peer-a'];
    if (pkg.peerDependenciesMeta) delete pkg.peerDependenciesMeta['@types/pnpm.e2e__peer-a'];
  }
  return pkg;
} } };
",
            )
            .expect("write pnpmfile");
        }
        if hook_action == "add" {
            fs::write(
                fixture.workspace.join(".pnpmfile.cjs"),
                r"
module.exports = { hooks: { readPackage(pkg) {
  if (pkg.name === 'library') pkg.peerDependencies = { '@pnpm.e2e/peer-a': '^1.0.0' };
  return pkg;
} } };
",
            )
            .expect("write pnpmfile");
        }
        let library = fixture.project(
            "library",
            "library",
            ManifestDeps { dev: &[("@pnpm.e2e/peer-a", "1.0.0")], ..ManifestDeps::default() },
        );
        crate::_utils::write_manifest_value(
            &library,
            &serde_json::json!({
                "name": "library", "version": "1.0.0",
                "peerDependencies": if hook_action == "add" { serde_json::json!({}) } else { serde_json::json!({ "@pnpm.e2e/peer-a": "^1.0.0" }) },
                "devDependencies": { "@pnpm.e2e/peer-a": "1.0.0" }
            }),
        );
        fixture.project(
            "consumer",
            "consumer",
            ManifestDeps {
                prod: &[("library", "workspace:*"), ("@pnpm.e2e/peer-a", "1.0.1")],
                ..ManifestDeps::default()
            },
        );
        fixture.run(["install"]);
        fixture.run(["install", "--frozen-lockfile"]);
    }
}
