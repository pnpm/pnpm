//! One lockfile key per registry: a plain range routed by scope and a
//! `<prefix>:` specifier that reach the same registry record one
//! `packages:` entry, one snapshot, and one copy in `node_modules/.pnpm`.

use crate::_utils::{append_workspace_yaml_key, pacquet_in, read_lockfile};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{ImporterDepVersion, Lockfile, PkgName, SnapshotDepRef};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    registry::TestRegistry,
};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};
use tempfile::TempDir;

/// `@pnpm.e2e/foobar@100.0.0` declares `@pnpm.e2e/foo: ^100.0.0`, a plain
/// range the `@pnpm.e2e` scope routes.
const PARENT: &str = "@pnpm.e2e/foobar";
const CHILD: &str = "@pnpm.e2e/foo";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest = format!(
        r#"{{ "name": "test-registry-prefix-keys", "version": "1.0.0", "dependencies": {dependencies} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

fn lockfile(workspace: &Path) -> Lockfile {
    read_lockfile(&workspace.join(Lockfile::FILE_NAME))
}

/// The sorted `packages:` keys recorded for `name`.
fn package_keys(lockfile: &Lockfile, name: &str) -> Vec<String> {
    let mut keys: Vec<String> = lockfile.packages
        .iter()
        .flatten()
        .filter(|(key, _)| key.name.to_string() == name)
        .map(|(key, _)| key.to_string())
        .collect();
    keys.sort();
    keys
}

/// The sorted `snapshots:` keys recorded for `name`.
fn snapshot_keys(lockfile: &Lockfile, name: &str) -> Vec<String> {
    let mut keys: Vec<String> = lockfile.snapshots
        .iter()
        .flatten()
        .filter(|(key, _)| key.name.to_string() == name)
        .map(|(key, _)| key.to_string())
        .collect();
    keys.sort();
    keys
}

/// The `node_modules/.pnpm` directories holding a copy of `name`.
fn virtual_store_copies(workspace: &Path, name: &str) -> Vec<String> {
    let prefix = format!("{}@", name.replace('/', "+"));
    let mut copies: Vec<String> = fs::read_dir(workspace.join("node_modules/.pnpm"))
        .expect("read node_modules/.pnpm")
        .map(|entry| {
            entry
                .expect("read entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|entry| entry.starts_with(&prefix))
        .collect();
    copies.sort();
    copies
}

/// The lockfile's `(specifier, version)` for a root dependency.
fn importer_entry(lockfile: &Lockfile, alias: &str) -> (String, String) {
    let alias: PkgName = alias.parse().expect("parse alias");
    let entry = lockfile.importers
        .get(Lockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.dependencies.as_ref())
        .and_then(|dependencies| dependencies.get(&alias))
        .unwrap_or_else(|| panic!("root importer records {alias}"));
    (entry.specifier.clone(), entry.version.to_string())
}

fn assert_single_child_copy(workspace: &Path, expected_key: &str) {
    let lockfile = lockfile(workspace);
    assert_eq!(package_keys(&lockfile, CHILD), [expected_key]);
    assert_eq!(snapshot_keys(&lockfile, CHILD), [expected_key]);
    assert_eq!(virtual_store_copies(workspace, CHILD).len(), 1);
}

/// `namedRegistries` prefix plus a `registries` scope route to one URL.
/// The prefix is spelled without the trailing slash the scope route and
/// `.npmrc` carry, which must not make it a different registry.
#[test]
fn plain_scoped_range_and_prefixed_range_share_one_key_with_named_registries() {
    let (root, workspace, anchor) = setup();
    let registry = anchor.mock_instance.url();
    append_workspace_yaml_key(
        &workspace,
        "namedRegistries",
        format!("{{ work: '{}' }}", registry.trim_end_matches('/')),
    );
    append_workspace_yaml_key(&workspace, "registries", format!("{{ '@pnpm.e2e': '{registry}' }}"));
    write_manifest(
        &workspace,
        r#"{ "@pnpm.e2e/foobar": "work:100.0.0", "@pnpm.e2e/foo": "work:^100.0.0" }"#,
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_single_child_copy(&workspace, "@pnpm.e2e/foo@work:100.1.0");
    let lockfile = lockfile(&workspace);
    assert_eq!(package_keys(&lockfile, PARENT), ["@pnpm.e2e/foobar@work:100.0.0"]);
    assert_eq!(
        importer_entry(&lockfile, CHILD),
        ("work:^100.0.0".to_string(), "work:100.1.0".to_string()),
    );

    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_single_child_copy(&workspace, "@pnpm.e2e/foo@work:100.1.0");

    drop((root, anchor));
}

/// The URL-keyed `registries` entry declaring both `scopes` and `prefix`.
#[test]
fn plain_scoped_range_and_prefixed_range_share_one_key_with_registry_declaration() {
    let (root, workspace, anchor) = setup();
    let registry = anchor.mock_instance.url();
    append_workspace_yaml_key(
        &workspace,
        "registries",
        format!("{{ '{registry}': {{ scopes: ['@pnpm.e2e'], prefix: work }} }}"),
    );
    write_manifest(
        &workspace,
        r#"{ "@pnpm.e2e/foobar": "work:100.0.0", "@pnpm.e2e/foo": "work:^100.0.0" }"#,
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_single_child_copy(&workspace, "@pnpm.e2e/foo@work:100.1.0");
    let lockfile = lockfile(&workspace);
    assert_eq!(package_keys(&lockfile, PARENT), ["@pnpm.e2e/foobar@work:100.0.0"]);

    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_single_child_copy(&workspace, "@pnpm.e2e/foo@work:100.1.0");

    drop((root, anchor));
}

/// The same name and version served by two registries stays two entries:
/// one named by its prefix, one plain under the scope route to the other
/// registry.
#[test]
fn same_version_from_two_registries_keeps_separate_keys() {
    let (root, workspace, anchor) = setup();
    let other = TestRegistry::start_with_own_storage(root.path());
    append_workspace_yaml_key(
        &workspace,
        "namedRegistries",
        format!("{{ work: '{}' }}", anchor.mock_instance.url()),
    );
    append_workspace_yaml_key(
        &workspace,
        "registries",
        format!("{{ '@pnpm.e2e': '{}' }}", other.url()),
    );
    write_manifest(
        &workspace,
        r#"{ "@pnpm.e2e/foo": "work:100.1.0", "foo-plain": "npm:@pnpm.e2e/foo@100.1.0" }"#,
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let lockfile = lockfile(&workspace);
    assert_eq!(
        package_keys(&lockfile, CHILD),
        ["@pnpm.e2e/foo@100.1.0", "@pnpm.e2e/foo@work:100.1.0"],
    );
    assert_eq!(virtual_store_copies(&workspace, CHILD).len(), 2);

    drop((root, anchor, other));
}

/// Several declared prefixes for one registry key its packages by the
/// lexicographically smallest one, whichever prefix a dependency spells.
#[test]
fn several_prefixes_for_one_registry_key_by_the_smallest() {
    let (root, workspace, anchor) = setup();
    let registry = anchor.mock_instance.url();
    append_workspace_yaml_key(
        &workspace,
        "namedRegistries",
        format!("{{ zeta: '{registry}', work: '{registry}' }}"),
    );
    write_manifest(
        &workspace,
        r#"{ "@pnpm.e2e/foobar": "zeta:100.0.0", "@pnpm.e2e/foo": "work:^100.0.0" }"#,
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_single_child_copy(&workspace, "@pnpm.e2e/foo@work:100.1.0");
    let lockfile = lockfile(&workspace);
    assert_eq!(package_keys(&lockfile, PARENT), ["@pnpm.e2e/foobar@work:100.0.0"]);
    assert_eq!(
        importer_entry(&lockfile, PARENT),
        ("zeta:100.0.0".to_string(), "work:100.0.0".to_string()),
    );
    let written = fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read lockfile");

    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read lockfile"),
        written,
        "a repeated install keeps the lockfile as written",
    );

    drop((root, anchor));
}

/// Rewrite the lockfile to the shape written before plain ranges were
/// keyed by their registry: the child duplicated under a plain key, which
/// the parent's snapshot points at.
fn duplicate_child_under_plain_key(workspace: &Path, version: &str) {
    let mut lockfile = lockfile(workspace);
    let qualified_key = format!("@pnpm.e2e/foo@work:{version}").parse().expect("parse key");
    let plain_key: pnpm_lockfile::PkgNameVerPeer =
        format!("@pnpm.e2e/foo@{version}").parse().expect("parse key");
    let packages = lockfile.packages.as_mut().expect("packages");
    let metadata = packages
        .get(&qualified_key)
        .expect("qualified child entry")
        .clone();
    packages.insert(plain_key.clone(), metadata);
    let snapshots = lockfile.snapshots.as_mut().expect("snapshots");
    let snapshot = snapshots
        .get(&qualified_key)
        .expect("qualified child snapshot")
        .clone();
    snapshots.insert(plain_key, snapshot);
    let parent_key = "@pnpm.e2e/foobar@work:100.0.0".parse().expect("parse key");
    let parent = snapshots.get_mut(&parent_key).expect("parent snapshot");
    let child: PkgName = CHILD.parse().expect("parse child name");
    parent.dependencies
        .as_mut()
        .expect("parent dependencies")
        .insert(child, SnapshotDepRef::Plain(version.parse().expect("parse version")));
    fs::write(
        workspace.join(Lockfile::FILE_NAME),
        lockfile.to_yaml_string().expect("serialize lockfile"),
    )
    .expect("write lockfile");
}

/// A lockfile written before plain ranges were keyed by their registry
/// still installs frozen, as recorded. The next resolving install merges
/// the duplicate under the prefixed key.
#[test]
fn frozen_install_keeps_an_older_duplicate_and_a_resolving_install_merges_it() {
    let (root, workspace, anchor) = setup();
    let registry = anchor.mock_instance.url();
    append_workspace_yaml_key(&workspace, "namedRegistries", format!("{{ work: '{registry}' }}"));
    append_workspace_yaml_key(&workspace, "registries", format!("{{ '@pnpm.e2e': '{registry}' }}"));
    write_manifest(
        &workspace,
        r#"{ "@pnpm.e2e/foobar": "work:100.0.0", "@pnpm.e2e/foo": "work:^100.0.0" }"#,
    );
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    duplicate_child_under_plain_key(&workspace, "100.1.0");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    let lockfile_after_frozen = lockfile(&workspace);
    assert_eq!(
        package_keys(&lockfile_after_frozen, CHILD),
        ["@pnpm.e2e/foo@100.1.0", "@pnpm.e2e/foo@work:100.1.0"],
    );
    assert_eq!(virtual_store_copies(&workspace, CHILD).len(), 2);

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let merged = lockfile(&workspace);
    assert_eq!(package_keys(&merged, CHILD), ["@pnpm.e2e/foo@work:100.1.0"]);
    assert_eq!(snapshot_keys(&merged, CHILD), ["@pnpm.e2e/foo@work:100.1.0"]);
    // Both edges link the one copy. The directory the plain key named stays
    // behind as an orphan, which an install does not remove.
    let root_copy =
        fs::canonicalize(workspace.join("node_modules").join(CHILD)).expect("resolve root link");
    let parent_dir =
        fs::canonicalize(workspace.join("node_modules").join(PARENT)).expect("resolve parent link");
    let nested_copy = fs::canonicalize(
        parent_dir
            .parent()
            .expect("the parent's scope directory")
            .join("foo"),
    )
    .expect("resolve nested link");
    assert_eq!(root_copy, nested_copy);
    assert!(root_copy.to_string_lossy().contains("work"), "{}", root_copy.display());

    drop((root, anchor));
}

/// The commands that read the merged keys back.
#[test]
fn inspection_commands_read_the_merged_keys() {
    let (root, workspace, anchor) = setup();
    let registry = anchor.mock_instance.url();
    append_workspace_yaml_key(&workspace, "namedRegistries", format!("{{ work: '{registry}' }}"));
    append_workspace_yaml_key(&workspace, "registries", format!("{{ '@pnpm.e2e': '{registry}' }}"));
    write_manifest(
        &workspace,
        r#"{ "@pnpm.e2e/foobar": "work:100.0.0", "@pnpm.e2e/foo": "work:^100.0.0" }"#,
    );
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    pacquet_in(&workspace)
        .with_args(["dedupe", "--check"])
        .assert()
        .success();

    let outdated = pacquet_in(&workspace)
        .with_args(["outdated", "--json"])
        .output()
        .expect("run outdated");
    assert!(
        outdated.status.success(),
        "nothing is outdated: {}",
        String::from_utf8_lossy(&outdated.stderr),
    );

    let licenses = pacquet_in(&workspace)
        .with_args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses");
    assert!(licenses.status.success(), "{}", String::from_utf8_lossy(&licenses.stderr));
    let report: serde_json::Value =
        serde_json::from_slice(&licenses.stdout).expect("parse licenses JSON");
    let foo_records: Vec<&serde_json::Value> = report
        .as_object()
        .expect("licenses by license")
        .values()
        .flat_map(|packages| {
            packages
                .as_array()
                .expect("packages")
                .iter()
        })
        .filter(|package| package["name"] == CHILD)
        .collect();
    assert_eq!(foo_records.len(), 1, "{report}");
    // A license record names the version the way its lockfile key does.
    assert_eq!(foo_records[0]["versions"], serde_json::json!(["work:100.1.0"]));

    let sbom = pacquet_in(&workspace)
        .with_args(["sbom", "--sbom-format", "cyclonedx"])
        .output()
        .expect("run sbom");
    assert!(sbom.status.success(), "{}", String::from_utf8_lossy(&sbom.stderr));
    let sbom: serde_json::Value = serde_json::from_slice(&sbom.stdout).expect("parse SBOM");
    let foo_components = sbom["components"]
        .as_array()
        .expect("components")
        .iter()
        .filter(|component| component["name"] == "foo" && component["group"] == "@pnpm.e2e")
        .count();
    assert_eq!(foo_components, 1, "{sbom}");

    drop((root, anchor));
}

/// Rewrite the lockfile to the shape written before prefixed specifiers were
/// keyed by their registry: the root's child under a plain key, pinned at
/// `version`, with the root's specifier widened to a range.
fn widen_root_child_to_a_plain_pin(workspace: &Path, version: &str) {
    let mut lockfile = lockfile(workspace);
    let child: PkgName = CHILD.parse().expect("parse child name");
    let entry = lockfile.importers
        .get_mut(Lockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.dependencies.as_mut())
        .and_then(|dependencies| dependencies.get_mut(&child))
        .expect("root records the child");
    entry.specifier = "work:^100.0.0".to_string();
    entry.version = ImporterDepVersion::Regular(version.parse().expect("parse version"));
    let qualified_key: pnpm_lockfile::PkgNameVerPeer =
        format!("@pnpm.e2e/foo@work:{version}").parse().expect("parse key");
    let plain_key: pnpm_lockfile::PkgNameVerPeer =
        format!("@pnpm.e2e/foo@{version}").parse().expect("parse key");
    let packages = lockfile.packages.as_mut().expect("packages");
    let metadata = packages.remove(&qualified_key).expect("qualified child entry");
    packages.insert(plain_key.clone(), metadata);
    let snapshots = lockfile.snapshots.as_mut().expect("snapshots");
    let snapshot = snapshots.remove(&qualified_key).expect("qualified child snapshot");
    snapshots.insert(plain_key, snapshot);
    fs::write(
        workspace.join(Lockfile::FILE_NAME),
        lockfile.to_yaml_string().expect("serialize lockfile"),
    )
    .expect("write lockfile");
}

/// Re-keying keeps the pinned version: the range admits 100.1.0, but the
/// lockfile pins 100.0.0 and the resolving install only renames its key.
#[test]
fn a_resolving_install_re_keys_without_bumping_the_pinned_version() {
    let (root, workspace, anchor) = setup();
    let registry = anchor.mock_instance.url();
    append_workspace_yaml_key(&workspace, "namedRegistries", format!("{{ work: '{registry}' }}"));
    append_workspace_yaml_key(&workspace, "registries", format!("{{ '@pnpm.e2e': '{registry}' }}"));
    write_manifest(&workspace, r#"{ "@pnpm.e2e/foo": "work:100.0.0" }"#);
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_single_child_copy(&workspace, "@pnpm.e2e/foo@work:100.0.0");
    widen_root_child_to_a_plain_pin(&workspace, "100.0.0");
    write_manifest(&workspace, r#"{ "@pnpm.e2e/foo": "work:^100.0.0" }"#);

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let merged = lockfile(&workspace);
    assert_eq!(package_keys(&merged, CHILD), ["@pnpm.e2e/foo@work:100.0.0"]);
    assert_eq!(
        importer_entry(&merged, CHILD),
        ("work:^100.0.0".to_string(), "work:100.0.0".to_string()),
    );

    drop((root, anchor));
}

/// A transitive plain pin survives the re-key too: the parent's `^100.0.0`
/// stays on the recorded 100.0.0 although 100.1.0 is published.
#[test]
fn a_resolving_install_re_keys_a_transitive_pin_without_bumping_it() {
    let (root, workspace, anchor) = setup();
    let registry = anchor.mock_instance.url();
    append_workspace_yaml_key(&workspace, "namedRegistries", format!("{{ work: '{registry}' }}"));
    append_workspace_yaml_key(&workspace, "registries", format!("{{ '@pnpm.e2e': '{registry}' }}"));
    write_manifest(
        &workspace,
        r#"{ "@pnpm.e2e/foobar": "work:100.0.0", "@pnpm.e2e/foo": "work:100.0.0" }"#,
    );
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_single_child_copy(&workspace, "@pnpm.e2e/foo@work:100.0.0");
    duplicate_child_under_plain_key(&workspace, "100.0.0");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let merged = lockfile(&workspace);
    assert_eq!(package_keys(&merged, CHILD), ["@pnpm.e2e/foo@work:100.0.0"]);
    assert_eq!(snapshot_keys(&merged, CHILD), ["@pnpm.e2e/foo@work:100.0.0"]);

    drop((root, anchor));
}

/// A `jsr:` specifier resolves through the `@jsr` scope; when that registry
/// declares a prefix, its key carries it like every other route to it.
#[test]
fn jsr_specifiers_share_the_key_of_the_registry_prefix() {
    let (root, workspace, anchor) = setup();
    let registry = anchor.mock_instance.url();
    let npmrc = fs::read_to_string(&anchor.npmrc_path).expect("read .npmrc");
    fs::write(&anchor.npmrc_path, format!("{npmrc}@jsr:registry={registry}\n"))
        .expect("write .npmrc");
    append_workspace_yaml_key(&workspace, "namedRegistries", format!("{{ work: '{registry}' }}"));
    write_manifest(
        &workspace,
        r#"{ "@pnpm-e2e/bar": "jsr:@pnpm-e2e/bar@2.0.0", "bar-work": "work:@jsr/pnpm-e2e__bar@2.0.0" }"#,
    );

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let lockfile = lockfile(&workspace);
    assert_eq!(package_keys(&lockfile, "@jsr/pnpm-e2e__bar"), ["@jsr/pnpm-e2e__bar@work:2.0.0"]);
    assert_eq!(snapshot_keys(&lockfile, "@jsr/pnpm-e2e__bar"), ["@jsr/pnpm-e2e__bar@work:2.0.0"]);

    drop((root, anchor));
}
