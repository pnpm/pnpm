//! A devDependency that satisfies a production package's optional peer is not
//! a production dependency: <https://github.com/pnpm/pnpm/issues/15344>.
//!
//! `@pnpm.e2e/abc-optional-peers` has a required peer `@pnpm.e2e/peer-a` and
//! optional peers `@pnpm.e2e/peer-b` and `@pnpm.e2e/peer-c`.

use crate::_utils::{ManifestDeps, WorkspaceFixture, assert_success, read_lockfile};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const ABC: &str = "@pnpm.e2e/abc-optional-peers";
const PEER_A: &str = "@pnpm.e2e/peer-a";
const PEER_C: &str = "@pnpm.e2e/peer-c";

const PROD_ABC: &[(&str, &str)] = &[(ABC, "1.0.0")];
const DEV_PEERS: &[(&str, &str)] = &[(PEER_A, "1.0.0"), (PEER_C, "1.0.0")];

/// A project whose production `abc-optional-peers` has both peers satisfied
/// by its own devDependencies, resolved into `pnpm-lock.yaml` only.
fn dev_provided_peers() -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: PROD_ABC, dev: DEV_PEERS, ..ManifestDeps::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    fixture
}

fn pnpm_at(fixture: &WorkspaceFixture, cwd: &Path, args: &[&str]) -> Output {
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(cwd)
        .with_env("PNPM_CONFIG_REGISTRY", fixture.registry.mock_instance.url())
        .with_args(args)
        .output()
        .expect("run pnpm");
    assert_success(&output);
    output
}

fn pnpm(fixture: &WorkspaceFixture, args: &[&str]) -> Output {
    pnpm_at(fixture, &fixture.workspace, args)
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("parse the JSON output")
}

/// The virtual-store slot the `modules_dir` link to the scoped package `name`
/// points into. The slot's name is not predictable: a short
/// `virtualStoreDirMaxLength`, the Windows default, hashes it.
fn slot_of(modules_dir: &Path, name: &str) -> PathBuf {
    let package_dir = fs::canonicalize(modules_dir.join(name))
        .unwrap_or_else(|error| panic!("resolve the {name} link: {error}"));
    package_dir
        .ancestors()
        .nth(3)
        .expect("a package inside a slot has a slot")
        .to_path_buf()
}

/// The virtual-store slot of the `abc-optional-peers` that `importer` depends
/// on directly.
fn abc_slot(importer: &Path) -> PathBuf {
    slot_of(&importer.join("node_modules"), ABC)
}

/// Whether the slot links `name` at all, a dangling link included.
fn links(slot: &Path, name: &str) -> bool {
    fs::symlink_metadata(slot.join("node_modules").join(name)).is_ok()
}

fn has_slot(workspace: &Path, name: &str, version: &str) -> bool {
    workspace
        .join("node_modules/.pnpm")
        .join(format!("{}@{version}", name.replace('/', "+")))
        .exists()
}

/// The aliases the current lockfile records on the `abc-optional-peers`
/// snapshot.
fn current_abc_aliases(workspace: &Path) -> Vec<String> {
    let current = read_lockfile(&workspace.join("node_modules/.pnpm/lock.yaml"));
    let (_, snapshot) = current.snapshots
        .iter()
        .flatten()
        .find(|(key, _)| {
            key.to_string()
                .starts_with(&format!("{ABC}@"))
        })
        .expect("the current lockfile records abc-optional-peers");
    let mut aliases = [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
        .flat_map(|entries| entries.keys())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    aliases.sort();
    aliases
}

#[test]
fn a_frozen_prod_install_leaves_out_an_optional_peer_only_a_dev_dependency_provides() {
    let fixture = dev_provided_peers();
    let workspace = &fixture.workspace;

    pnpm(&fixture, &["install", "--prod", "--frozen-lockfile"]);

    let slot = abc_slot(workspace);
    assert!(!has_slot(workspace, PEER_C, "1.0.0"), "the optional peer must not be installed");
    assert!(!links(&slot, PEER_C), "abc-optional-peers must not link the optional peer");
    assert!(has_slot(workspace, PEER_A, "1.0.0"), "the required peer must be installed");
    assert!(links(&slot, PEER_A), "abc-optional-peers must link the required peer");
    assert_eq!(current_abc_aliases(workspace), [PEER_A]);

    pnpm(&fixture, &["install", "--frozen-lockfile"]);

    let slot = abc_slot(workspace);
    assert!(
        slot.join("node_modules")
            .join(PEER_C)
            .exists(),
        "a full install must link the optional peer again",
    );
    assert_eq!(current_abc_aliases(workspace), [PEER_A, PEER_C]);
}

#[test]
fn a_prod_install_after_a_full_install_unlinks_the_optional_peer() {
    let fixture = dev_provided_peers();
    let workspace = &fixture.workspace;

    pnpm(&fixture, &["install", "--frozen-lockfile"]);
    assert!(
        abc_slot(workspace)
            .join("node_modules")
            .join(PEER_C)
            .exists(),
    );

    pnpm(&fixture, &["install", "--prod", "--frozen-lockfile"]);

    let slot = abc_slot(workspace);
    assert!(!links(&slot, PEER_C), "the optional peer's link must be removed, not left dangling");
    assert!(links(&slot, PEER_A));
}

#[test]
fn a_prod_install_keeps_a_required_peer_no_importer_lists() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest("root", ManifestDeps { prod: PROD_ABC, ..ManifestDeps::default() });

    pnpm(&fixture, &["install", "--prod"]);

    let slot = abc_slot(&fixture.workspace);
    assert!(
        slot.join("node_modules")
            .join(PEER_A)
            .exists(),
        "the auto-installed peer stays",
    );
}

/// `abc-optional-peers-parent` depends on `abc-optional-peers` and on every
/// one of its peers, so the optional peer is installed as the parent's own
/// dependency, and `abc-optional-peers` links it.
#[test]
fn a_prod_install_keeps_an_optional_peer_a_production_package_depends_on() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest(
        "root",
        ManifestDeps {
            prod: &[("@pnpm.e2e/abc-optional-peers-parent", "1.0.0")],
            dev: &[(PEER_C, "1.0.1")],
            ..ManifestDeps::default()
        },
    );
    fixture.run(["install", "--lockfile-only"]);

    pnpm(&fixture, &["install", "--prod", "--frozen-lockfile"]);

    let parent =
        slot_of(&fixture.workspace.join("node_modules"), "@pnpm.e2e/abc-optional-peers-parent");
    let slot = slot_of(&parent.join("node_modules"), ABC);
    assert!(
        slot.join("node_modules")
            .join(PEER_C)
            .exists(),
        "the optional peer stays linked",
    );
}

/// The root lists the optional peer as a devDependency, and the project that
/// depends on `abc-optional-peers` does not list it at all, so the peer is
/// resolved from the root.
fn root_provided_optional_peer() -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest(
        "root",
        ManifestDeps { dev: &[(PEER_C, "1.0.0")], ..ManifestDeps::default() },
    );
    fixture.project(
        "app",
        "app",
        ManifestDeps { prod: &[(ABC, "1.0.0"), (PEER_A, "1.0.0")], ..ManifestDeps::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let wanted = fixture.wanted();
    assert!(
        wanted.snapshots
            .iter()
            .flatten()
            .any(|(key, _)| key.to_string().starts_with(ABC) && key.to_string().contains(PEER_C)),
        "the root must provide the optional peer: {wanted:#?}",
    );
    fixture
}

#[test]
fn a_prod_install_drops_an_optional_peer_the_workspace_root_provides() {
    let fixture = root_provided_optional_peer();

    pnpm(&fixture, &["install", "--prod", "--frozen-lockfile"]);

    assert!(!has_slot(&fixture.workspace, PEER_C, "1.0.0"));
    assert!(!links(&abc_slot(&fixture.workspace.join("packages/app")), PEER_C));
}

#[test]
fn a_prod_install_keeps_a_root_provided_optional_peer_without_resolve_peers_from_workspace_root() {
    let fixture = root_provided_optional_peer();
    fixture.append_workspace_yaml("resolvePeersFromWorkspaceRoot: false\n");

    pnpm(&fixture, &["install", "--prod", "--frozen-lockfile"]);

    assert!(has_slot(&fixture.workspace, PEER_C, "1.0.0"));
    assert!(
        abc_slot(&fixture.workspace.join("packages/app"))
            .join("node_modules")
            .join(PEER_C)
            .exists(),
    );
}

#[test]
fn a_hoisted_prod_install_leaves_out_an_optional_peer_only_a_dev_dependency_provides() {
    let fixture = dev_provided_peers();
    fixture.append_workspace_yaml("nodeLinker: hoisted\n");

    pnpm(&fixture, &["install", "--prod", "--frozen-lockfile"]);

    let modules = fixture.workspace.join("node_modules");
    assert!(modules.join(ABC).exists());
    assert!(modules.join(PEER_A).exists(), "the required peer is installed");
    assert!(!modules.join(PEER_C).exists(), "the optional peer is not installed");
}

#[test]
fn fetch_prod_leaves_out_an_optional_peer_only_a_dev_dependency_provides() {
    let fixture = dev_provided_peers();

    pnpm(&fixture, &["fetch", "--prod"]);

    assert!(has_slot(&fixture.workspace, PEER_A, "1.0.0"));
    assert!(!has_slot(&fixture.workspace, PEER_C, "1.0.0"));
}

fn deploy_workspace(
    root: ManifestDeps<'_>,
    app_dev: &[(&'static str, &'static str)],
) -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("injectWorkspacePackages: true\n");
    fixture.write_root_manifest("root", root);
    fixture.project(
        "app",
        "app",
        ManifestDeps {
            prod: &[(ABC, "1.0.0"), (PEER_A, "1.0.0")],
            dev: app_dev,
            ..ManifestDeps::default()
        },
    );
    fixture.run(["install"]);
    fixture
}

fn assert_deploy_leaves_out_the_optional_peer(fixture: &WorkspaceFixture) {
    pnpm(fixture, &["--filter", "app", "deploy", "--prod", "deploy"]);

    let deploy_dir = fixture.workspace.join("deploy");
    let deployed = read_lockfile(&deploy_dir.join("pnpm-lock.yaml"));
    let keys = deployed.snapshots
        .iter()
        .flatten()
        .map(|(key, _)| key.to_string())
        .collect::<Vec<_>>();
    assert!(
        !keys
            .iter()
            .any(|key| key.starts_with(PEER_C)),
        "the deploy must not ship the optional peer: {keys:#?}",
    );
    assert!(
        keys.iter()
            .any(|key| key.starts_with(PEER_A)),
    );
    assert!(!has_slot(&deploy_dir, PEER_C, "1.0.0"));
    assert!(!links(&abc_slot(&deploy_dir), PEER_C));
}

#[test]
fn deploy_prod_leaves_out_an_optional_peer_the_project_dev_dependencies_provide() {
    let fixture = deploy_workspace(ManifestDeps::default(), &[(PEER_C, "1.0.0")]);
    assert_deploy_leaves_out_the_optional_peer(&fixture);
}

#[test]
fn deploy_prod_leaves_out_an_optional_peer_the_workspace_root_provides() {
    let fixture = deploy_workspace(
        ManifestDeps { dev: &[(PEER_C, "1.0.0")], ..ManifestDeps::default() },
        &[],
    );
    assert_deploy_leaves_out_the_optional_peer(&fixture);
}

/// The names `list --json` shows under the root project's
/// `abc-optional-peers`.
fn listed_abc_children(fixture: &WorkspaceFixture, extra_args: &[&str]) -> Vec<String> {
    let args = [&["list", "--json", "--depth", "1"][..], extra_args].concat();
    let listed = json(&pnpm(fixture, &args));
    let mut names = listed[0]["dependencies"][ABC]["dependencies"]
        .as_object()
        .map(|children| {
            children
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn list_prod_leaves_out_an_optional_peer_only_a_dev_dependency_provides() {
    let fixture = dev_provided_peers();
    pnpm(&fixture, &["install", "--frozen-lockfile"]);

    assert_eq!(listed_abc_children(&fixture, &[]), [PEER_A, PEER_C]);
    assert_eq!(listed_abc_children(&fixture, &["--prod"]), [PEER_A]);
}

#[test]
fn why_prod_finds_no_path_to_an_optional_peer_only_a_dev_dependency_provides() {
    let fixture = dev_provided_peers();
    pnpm(&fixture, &["install", "--frozen-lockfile"]);

    let full = String::from_utf8(pnpm(&fixture, &["why", PEER_C]).stdout).unwrap();
    assert!(full.contains(ABC), "without --prod, abc-optional-peers leads to the peer:\n{full}");
    let prod = String::from_utf8(pnpm(&fixture, &["why", "--prod", PEER_C]).stdout).unwrap();
    assert!(!prod.contains(PEER_C), "--prod finds no path to the peer:\n{prod}");
}

fn licensed_names(fixture: &WorkspaceFixture, extra_args: &[&str]) -> Vec<String> {
    let args = [&["licenses", "list", "--json"][..], extra_args].concat();
    let report = json(&pnpm(fixture, &args));
    let mut names = report
        .as_object()
        .expect("licenses JSON is an object")
        .values()
        .flat_map(|packages| {
            packages
                .as_array()
                .expect("a license's package list")
                .iter()
        })
        .map(|package| {
            package["name"]
                .as_str()
                .expect("package name")
                .to_string()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn licenses_prod_leaves_out_an_optional_peer_only_a_dev_dependency_provides() {
    let fixture = dev_provided_peers();
    pnpm(&fixture, &["install", "--frozen-lockfile"]);

    assert_eq!(licensed_names(&fixture, &[]), [ABC, PEER_A, PEER_C]);
    assert_eq!(licensed_names(&fixture, &["--prod"]), [ABC, PEER_A]);
}

fn sbom(fixture: &WorkspaceFixture, extra_args: &[&str]) -> Value {
    let args = [&["sbom", "--sbom-format", "cyclonedx"][..], extra_args].concat();
    json(&pnpm(fixture, &args))
}

fn component_names(sbom: &Value) -> Vec<String> {
    let mut names = sbom["components"]
        .as_array()
        .expect("components")
        .iter()
        .map(|component| {
            component["name"]
                .as_str()
                .expect("component name")
                .to_string()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// The component refs `abc-optional-peers` depends on.
fn abc_depends_on(sbom: &Value) -> Vec<String> {
    let abc_ref = sbom["components"]
        .as_array()
        .expect("components")
        .iter()
        .find(|component| component["name"] == "abc-optional-peers")
        .and_then(|component| component["bom-ref"].as_str())
        .expect("abc-optional-peers has a bom-ref");
    sbom["dependencies"]
        .as_array()
        .expect("dependencies")
        .iter()
        .find(|entry| entry["ref"] == abc_ref)
        .and_then(|entry| entry["dependsOn"].as_array())
        .map(|refs| {
            refs.iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn sbom_prod_leaves_out_an_optional_peer_only_a_dev_dependency_provides() {
    let fixture = dev_provided_peers();
    pnpm(&fixture, &["install", "--frozen-lockfile"]);

    let full = sbom(&fixture, &[]);
    assert_eq!(component_names(&full), ["abc-optional-peers", "peer-a", "peer-c"]);
    assert!(
        abc_depends_on(&full)
            .iter()
            .any(|reference| reference.contains("peer-c")),
        "without --prod the relationship to the peer stays: {full:#}",
    );

    let prod = sbom(&fixture, &["--prod"]);
    assert_eq!(component_names(&prod), ["abc-optional-peers", "peer-a"]);
    assert!(
        !abc_depends_on(&prod)
            .iter()
            .any(|reference| reference.contains("peer-c")),
    );
}

/// A root production dependency is installed whenever the root is, so it does
/// not make the peer edge a dev-only one: the deploy still ships it.
#[test]
fn deploy_prod_ships_an_optional_peer_the_workspace_root_lists_as_a_production_dependency() {
    let fixture = deploy_workspace(
        ManifestDeps { prod: &[(PEER_C, "1.0.0")], ..ManifestDeps::default() },
        &[],
    );

    pnpm(&fixture, &["--filter", "app", "deploy", "--prod", "deploy"]);

    let deploy_dir = fixture.workspace.join("deploy");
    assert!(has_slot(&deploy_dir, PEER_C, "1.0.0"), "the deploy must ship the optional peer");
    assert!(
        abc_slot(&deploy_dir)
            .join("node_modules")
            .join(PEER_C)
            .exists(),
    );
}

#[test]
fn a_resolving_prod_install_leaves_out_an_optional_peer_only_a_dev_dependency_provides() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: PROD_ABC, dev: DEV_PEERS, ..ManifestDeps::default() },
    );

    pnpm(&fixture, &["install", "--prod"]);

    let workspace = &fixture.workspace;
    let slot = abc_slot(workspace);
    assert!(!has_slot(workspace, PEER_C, "1.0.0"), "the optional peer must not be installed");
    assert!(!links(&slot, PEER_C), "abc-optional-peers must not link the optional peer");
    assert!(has_slot(workspace, PEER_A, "1.0.0"), "the required peer must be installed");
    assert!(links(&slot, PEER_A));
    assert_eq!(current_abc_aliases(workspace), [PEER_A]);
}

/// The mirror image of `--prod`: a devDependency's optional peer that only a
/// production dependency provides is left out of a `--dev` install.
#[test]
fn a_dev_install_leaves_out_an_optional_peer_only_a_production_dependency_provides() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: DEV_PEERS, dev: PROD_ABC, ..ManifestDeps::default() },
    );
    fixture.run(["install", "--lockfile-only"]);

    pnpm(&fixture, &["install", "--dev", "--frozen-lockfile"]);

    let workspace = &fixture.workspace;
    let slot = abc_slot(workspace);
    assert!(!has_slot(workspace, PEER_C, "1.0.0"), "the optional peer must not be installed");
    assert!(!links(&slot, PEER_C));
    assert!(has_slot(workspace, PEER_A, "1.0.0"), "the required peer must be installed");
    assert!(links(&slot, PEER_A));
}
