use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{Lockfile, PkgName, ProjectSnapshot, SnapshotEntry};
use pacquet_modules_yaml::{Host, Modules, read_modules_manifest, write_modules_manifest};
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use pacquet_workspace_state::WorkspaceState;
use pretty_assertions::assert_eq;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const CATALOG_FOO: &str = "@pnpm.e2e/foo";
const HELLO: &str = "@pnpm.e2e/hello-world-js-bin";
const HELLO_PARENT: &str = "@pnpm.e2e/hello-world-js-bin-parent";
const NO_DEPS: &str = "@foo/no-deps";
const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";

#[derive(Clone, Copy, Default)]
struct ManifestDeps<'a> {
    prod: &'a [(&'a str, &'a str)],
    dev: &'a [(&'a str, &'a str)],
    optional: &'a [(&'a str, &'a str)],
}

struct FilteredWorkspace {
    _root: TempDir,
    workspace: PathBuf,
    registry: AddMockedRegistry,
}

impl FilteredWorkspace {
    fn new() -> Self {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let fixture = Self { _root: root, workspace, registry: npmrc_info };
        fixture.append_workspace_yaml("packages:\n  - 'packages/*'\n");
        fixture
    }

    fn append_workspace_yaml(&self, text: &str) {
        let path = self.workspace.join("pnpm-workspace.yaml");
        let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
        if !yaml.ends_with('\n') {
            yaml.push('\n');
        }
        yaml.push_str(text);
        fs::write(path, yaml).expect("write pnpm-workspace.yaml");
    }

    fn write_root_manifest(&self, name: &str, deps: ManifestDeps<'_>) {
        write_manifest(&self.workspace, name, deps);
    }

    fn project(&self, dir: &str, name: &str, deps: ManifestDeps<'_>) -> PathBuf {
        let project = self.workspace.join("packages").join(dir);
        write_manifest(&project, name, deps);
        project
    }

    fn command_at<I, S>(&self, cwd: &Path, args: I) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(cwd)
            .env("PNPM_CONFIG_REGISTRY", self.registry.mock_instance.url())
            .arg("--reporter=ndjson")
            .args(args)
            .output()
            .expect("run pacquet")
    }

    fn run<I, S>(&self, args: I) -> Vec<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_at(&self.workspace, args)
    }

    fn run_at<I, S>(&self, cwd: &Path, args: I) -> Vec<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.command_at(cwd, args);
        assert_success(&output);
        ndjson_records(&output)
    }

    fn wanted(&self) -> Lockfile {
        read_lockfile(&self.workspace.join("pnpm-lock.yaml"))
    }

    fn current(&self) -> Lockfile {
        read_lockfile(&self.workspace.join("node_modules/.pnpm/lock.yaml"))
    }

    fn modules(&self) -> Modules {
        read_modules_manifest::<Host>(&self.workspace.join("node_modules"))
            .expect("read .modules.yaml")
            .expect(".modules.yaml exists")
    }

    fn write_modules(&self, modules: Modules) {
        write_modules_manifest::<Host>(&self.workspace.join("node_modules"), modules)
            .expect("write .modules.yaml");
    }

    fn state(&self) -> WorkspaceState {
        let path = self.workspace.join("node_modules/.pnpm-workspace-state-v1.json");
        serde_json::from_str(&fs::read_to_string(path).expect("read workspace state"))
            .expect("parse workspace state")
    }

    fn package_map(&self) -> Value {
        let path = self.workspace.join("node_modules/.package-map.json");
        serde_json::from_str(&fs::read_to_string(path).expect("read package map"))
            .expect("parse package map")
    }

    fn slot(&self, name: &str, version: &str) -> PathBuf {
        self.workspace
            .join("node_modules/.pnpm")
            .join(format!("{}@{version}", name.replace('/', "+")))
    }
}

fn write_manifest(project: &Path, name: &str, deps: ManifestDeps<'_>) {
    fs::create_dir_all(project).expect("create project directory");
    let mut manifest = Map::from_iter([
        ("name".to_string(), Value::String(name.to_string())),
        ("version".to_string(), Value::String("1.0.0".to_string())),
        ("private".to_string(), Value::Bool(true)),
    ]);
    insert_dependency_group(&mut manifest, "dependencies", deps.prod);
    insert_dependency_group(&mut manifest, "devDependencies", deps.dev);
    insert_dependency_group(&mut manifest, "optionalDependencies", deps.optional);
    fs::write(
        project.join("package.json"),
        serde_json::to_string_pretty(&Value::Object(manifest)).expect("serialize package.json"),
    )
    .expect("write package.json");
}

fn insert_dependency_group(manifest: &mut Map<String, Value>, group: &str, deps: &[(&str, &str)]) {
    if deps.is_empty() {
        return;
    }
    manifest.insert(
        group.to_string(),
        Value::Object(
            deps.iter()
                .map(|(name, spec)| (name.to_string(), Value::String(spec.to_string())))
                .collect(),
        ),
    );
}

fn read_manifest(project: &Path) -> Value {
    serde_json::from_str(
        &fs::read_to_string(project.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json")
}

fn write_manifest_value(project: &Path, manifest: &Value) {
    fs::write(
        project.join("package.json"),
        serde_json::to_string_pretty(manifest).expect("serialize package.json"),
    )
    .expect("write package.json");
}

fn set_dependency(project: &Path, group: &str, name: &str, spec: &str) {
    let mut manifest = read_manifest(project);
    let object = manifest.as_object_mut().expect("manifest is an object");
    let dependencies = object.entry(group).or_insert_with(|| json!({}));
    dependencies
        .as_object_mut()
        .expect("dependency group is an object")
        .insert(name.to_string(), Value::String(spec.to_string()));
    write_manifest_value(project, &manifest);
}

fn set_version(project: &Path, version: &str) {
    let path = project.join("package.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read package.json"))
            .expect("parse package.json");
    manifest["version"] = Value::String(version.to_string());
    fs::write(path, serde_json::to_string_pretty(&manifest).expect("serialize package.json"))
        .expect("write package.json");
}

fn replace_dependencies(project: &Path, deps: &[(&str, &str)]) {
    let mut manifest = read_manifest(project);
    manifest["dependencies"] = Value::Object(
        deps.iter()
            .map(|(name, spec)| (name.to_string(), Value::String(spec.to_string())))
            .collect(),
    );
    write_manifest_value(project, &manifest);
}

fn dependency_spec(project: &Path, group: &str, name: &str) -> Option<String> {
    read_manifest(project)
        .get(group)
        .and_then(Value::as_object)
        .and_then(|dependencies| dependencies.get(name))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn read_lockfile(path: &Path) -> Lockfile {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read lockfile {}: {error}", path.display()));
    serde_saphyr::from_str(&contents)
        .unwrap_or_else(|error| panic!("parse lockfile {}: {error}\n{contents}", path.display()))
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn ndjson_records(output: &Output) -> Vec<Value> {
    [&output.stderr[..], &output.stdout[..]]
        .into_iter()
        .flat_map(|stream| {
            String::from_utf8_lossy(stream).lines().map(str::to_string).collect::<Vec<_>>()
        })
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

fn importing_started_count(records: &[Value]) -> usize {
    records
        .iter()
        .filter(|record| {
            record.get("name").and_then(Value::as_str) == Some("pnpm:stage")
                && record.get("stage").and_then(Value::as_str) == Some("importing_started")
        })
        .count()
}

fn initial_manifest_prefixes(records: &[Value]) -> Vec<String> {
    records
        .iter()
        .filter(|record| {
            record.get("name").and_then(Value::as_str) == Some("pnpm:package-manifest")
                && record.get("initial").is_some()
        })
        .filter_map(|record| record.get("prefix").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn importer<'a>(lockfile: &'a Lockfile, id: &str) -> &'a ProjectSnapshot {
    lockfile
        .importers
        .get(id)
        .unwrap_or_else(|| panic!("missing importer {id:?}: {:?}", lockfile.importers.keys()))
}

fn importer_version(lockfile: &Lockfile, id: &str, name: &str) -> String {
    let name: PkgName = name.parse().expect("parse package name");
    let snapshot = importer(lockfile, id);
    snapshot
        .dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(&name))
        .or_else(|| {
            snapshot.dev_dependencies.as_ref().and_then(|dependencies| dependencies.get(&name))
        })
        .or_else(|| {
            snapshot.optional_dependencies.as_ref().and_then(|dependencies| dependencies.get(&name))
        })
        .unwrap_or_else(|| panic!("missing dependency {name} in importer {id}"))
        .version
        .to_string()
}

fn importer_specifier(lockfile: &Lockfile, id: &str, name: &str) -> String {
    let name: PkgName = name.parse().expect("parse package name");
    importer(lockfile, id)
        .dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(&name))
        .unwrap_or_else(|| panic!("missing dependency {name} in importer {id}"))
        .specifier
        .clone()
}

fn importer_ids(lockfile: &Lockfile) -> BTreeSet<String> {
    lockfile.importers.keys().cloned().collect()
}

fn snapshot_entries(lockfile: &Lockfile, name: &str) -> Vec<(String, SnapshotEntry)> {
    lockfile
        .snapshots
        .as_ref()
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.to_string().starts_with(&format!("{name}@")))
        .map(|(key, snapshot)| (key.to_string(), snapshot.clone()))
        .collect()
}

fn has_snapshot(lockfile: &Lockfile, name: &str, version: &str) -> bool {
    lockfile.snapshots.as_ref().is_some_and(|snapshots| {
        snapshots.keys().any(|key| {
            let key = key.to_string();
            key == format!("{name}@{version}") || key.starts_with(&format!("{name}@{version}("))
        })
    })
}

fn has_link(project: &Path, name: &str) -> bool {
    is_symlink_or_junction(&project.join("node_modules").join(name)).unwrap_or(false)
}

fn canonical_path(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|error| panic!("canonicalize {}: {error}", path.display()))
        .to_string_lossy()
        .into_owned()
}

fn assert_stage_once(records: &[Value]) {
    assert_eq!(importing_started_count(records), 1, "one install pipeline must import once");
}

fn assert_full_wanted(lockfile: &Lockfile, ids: &[&str]) {
    assert_eq!(
        importer_ids(lockfile),
        ids.iter().map(ToString::to_string).collect(),
        "wanted lockfile must retain every real importer",
    );
}

#[test]
fn filtered_add_mutates_only_selected_importers() {
    let fixture = FilteredWorkspace::new();
    let selected_a = fixture.project("selected-a", "selected-a", ManifestDeps::default());
    let selected_b = fixture.project("selected-b", "selected-b", ManifestDeps::default());
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let before = fixture.wanted();
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    let records = fixture.run([
        "--filter",
        "selected-a",
        "--filter",
        "selected-b",
        "add",
        HELLO,
        "--lockfile-only",
    ]);
    let after = fixture.wanted();

    assert_eq!(dependency_spec(&selected_a, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(dependency_spec(&selected_b, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(importer(&after, "packages/unselected"), importer(&before, "packages/unselected"));
    assert_full_wanted(
        &after,
        &["packages/selected-a", "packages/selected-b", "packages/unselected"],
    );
    assert_stage_once(&records);
}

#[test]
fn filtered_update_mutates_only_selected_importers() {
    let fixture = FilteredWorkspace::new();
    let selected_a = fixture.project(
        "selected-a",
        "selected-a",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    let selected_b = fixture.project(
        "selected-b",
        "selected-b",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    for project in [&selected_a, &selected_b, &unselected] {
        set_dependency(project, "dependencies", DEP, "^100.0.0");
    }
    let before = fixture.wanted();
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    let records = fixture.run([
        "--filter",
        "selected-a",
        "--filter",
        "selected-b",
        "update",
        DEP,
        "--latest",
        "--lockfile-only",
    ]);
    let after = fixture.wanted();

    assert_eq!(dependency_spec(&selected_a, "dependencies", DEP).as_deref(), Some("^101.0.0"));
    assert_eq!(dependency_spec(&selected_b, "dependencies", DEP).as_deref(), Some("^101.0.0"));
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(importer(&after, "packages/unselected"), importer(&before, "packages/unselected"));
    assert_eq!(importer_version(&after, "packages/unselected", DEP), "100.0.0");
    assert_full_wanted(
        &after,
        &["packages/selected-a", "packages/selected-b", "packages/unselected"],
    );
    assert_stage_once(&records);
}

#[test]
fn filtered_remove_mutates_only_selected_importers() {
    let fixture = FilteredWorkspace::new();
    let selected_a = fixture.project(
        "selected-a",
        "selected-a",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let selected_b = fixture.project(
        "selected-b",
        "selected-b",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let before = fixture.wanted();
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    let records = fixture.run([
        "--filter",
        "selected-a",
        "--filter",
        "selected-b",
        "remove",
        HELLO,
        "--lockfile-only",
    ]);
    let after = fixture.wanted();

    assert_eq!(dependency_spec(&selected_a, "dependencies", HELLO), None);
    assert_eq!(dependency_spec(&selected_b, "dependencies", HELLO), None);
    assert_eq!(dependency_spec(&selected_b, "dependencies", PARENT).as_deref(), Some("100.0.0"));
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(importer(&after, "packages/unselected"), importer(&before, "packages/unselected"));
    assert_full_wanted(
        &after,
        &["packages/selected-a", "packages/selected-b", "packages/unselected"],
    );
    assert_stage_once(&records);
}

#[test]
fn filtered_update_from_selected_child_uses_discovered_manifest_as_source_of_truth() {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "0.0.0")], ..Default::default() },
    );
    let sibling = fixture.project(
        "sibling",
        "sibling",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let before = fixture.wanted();
    let sibling_manifest = fs::read(sibling.join("package.json")).expect("read manifest");
    fixture.run_at(&selected, ["--filter", ".", "update", HELLO, "--latest", "--lockfile-only"]);
    let after = fixture.wanted();

    assert_eq!(dependency_spec(&selected, "dependencies", HELLO).as_deref(), Some("1.0.0"));
    assert_eq!(importer_specifier(&after, "packages/selected", HELLO), "1.0.0");
    assert_eq!(importer_version(&after, "packages/selected", HELLO), "1.0.0");
    assert_eq!(fs::read(sibling.join("package.json")).expect("read manifest"), sibling_manifest,);
    assert_eq!(importer(&after, "packages/sibling"), importer(&before, "packages/sibling"));
    assert!(!after.importers.contains_key("."), "missing root must not become an importer");
}

#[test]
fn filtered_update_preserves_prior_importer_when_unselected_manifest_changed_externally() {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "0.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let before = fixture.wanted();
    let prior_importer = importer(&before, "packages/unselected").clone();
    let prior_parent = snapshot_entries(&before, PARENT);
    let prior_child = snapshot_entries(&before, DEP);
    replace_dependencies(&unselected, &[(HELLO_PARENT, "1.0.0")]);
    let external_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    fixture.run(["--filter", "selected", "update", HELLO, "--latest", "--lockfile-only"]);
    let after = fixture.wanted();

    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        external_manifest,
    );
    assert_eq!(importer(&after, "packages/unselected"), &prior_importer);
    assert_eq!(snapshot_entries(&after, PARENT), prior_parent);
    assert_eq!(snapshot_entries(&after, DEP), prior_child);
    assert!(snapshot_entries(&after, HELLO_PARENT).is_empty());
    assert_eq!(dependency_spec(&selected, "dependencies", HELLO).as_deref(), Some("1.0.0"));
    assert_eq!(importer_version(&after, "packages/selected", HELLO), "1.0.0");
}

fn compatible_update_scenario(selected_dir: &str, unselected_dir: &str) {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        selected_dir,
        "selected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        unselected_dir,
        "unselected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    set_dependency(&selected, "dependencies", DEP, "^100.0.0");
    set_dependency(&unselected, "dependencies", DEP, "^100.0.0");
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    fixture.run(["--filter", "selected", "update", DEP, "--lockfile-only"]);
    let lockfile = fixture.wanted();

    assert_eq!(importer_version(&lockfile, &format!("packages/{selected_dir}"), DEP), "100.1.0");
    assert_eq!(importer_version(&lockfile, &format!("packages/{unselected_dir}"), DEP), "100.0.0",);
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
}

#[test]
fn filtered_compatible_update_does_not_cross_importer_cache_boundaries() {
    compatible_update_scenario("a-selected", "z-unselected");
    compatible_update_scenario("z-selected", "a-unselected");
}

#[test]
fn filtered_compatible_update_keeps_workspace_manifest_preferences() {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(DEP, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    set_dependency(&selected, "dependencies", DEP, "^100.0.0");
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");

    fixture.run(["--filter", "selected", "update", DEP, "--lockfile-only"]);
    let lockfile = fixture.wanted();

    assert_eq!(importer_version(&lockfile, "packages/selected", DEP), "100.0.0");
    assert_eq!(importer_version(&lockfile, "packages/unselected", DEP), "100.0.0");
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
}

fn transitive_update_scenario(
    selected_dir: &str,
    unselected_dir: &str,
) -> (HashMap<pacquet_lockfile::PackageKey, SnapshotEntry>, String) {
    let fixture = FilteredWorkspace::new();
    let deps = || ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() };
    let selected = fixture.project(selected_dir, "selected", deps());
    let unselected = fixture.project(unselected_dir, "unselected", deps());
    fixture.run(["install", "--lockfile-only"]);
    let selected_manifest = fs::read(selected.join("package.json")).expect("read manifest");
    let unselected_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    let before = fixture.wanted();
    let selected_ref = importer_version(&before, &format!("packages/{selected_dir}"), PARENT);
    let unselected_ref = importer_version(&before, &format!("packages/{unselected_dir}"), PARENT);
    fixture.run(["--filter", "selected", "update", DEP, "--lockfile-only"]);
    let after = fixture.wanted();

    assert_eq!(fs::read(selected.join("package.json")).expect("read manifest"), selected_manifest);
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        unselected_manifest,
    );
    assert_eq!(importer_version(&after, &format!("packages/{selected_dir}"), PARENT), selected_ref,);
    assert_eq!(
        importer_version(&after, &format!("packages/{unselected_dir}"), PARENT),
        unselected_ref,
    );
    let parents = snapshot_entries(&after, PARENT);
    assert_eq!(parents.len(), 1, "one parent snapshot must have one canonical child set");
    let child_name: PkgName = DEP.parse().expect("parse child package name");
    let child = parents[0]
        .1
        .dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(&child_name))
        .expect("parent snapshot has child")
        .to_string();
    assert_eq!(child, "100.1.0");
    (after.snapshots.expect("snapshots exist"), child)
}

#[test]
fn filtered_transitive_update_keeps_one_canonical_shared_snapshot() {
    let (selected_first, selected_first_child) =
        transitive_update_scenario("a-selected", "z-unselected");
    let (unselected_first, unselected_first_child) =
        transitive_update_scenario("z-selected", "a-unselected");
    assert_eq!(selected_first_child, unselected_first_child);
    assert_eq!(selected_first, unselected_first);
}

fn assert_selected_isolated_closure(
    fixture: &FilteredWorkspace,
    selected: &Path,
    unselected: &Path,
) {
    assert!(has_link(selected, HELLO), "selected direct link must exist");
    assert!(fixture.slot(HELLO, "1.0.0").exists(), "selected virtual-store slot must exist");
    assert!(!unselected.join("node_modules").exists(), "unselected node_modules must be absent");
    assert!(!fixture.slot(PARENT, "100.0.0").exists(), "unselected direct slot must be absent");
    assert!(!fixture.slot(DEP, "100.1.0").exists(), "unselected transitive slot must be absent");
    let wanted = fixture.wanted();
    assert_full_wanted(&wanted, &["packages/selected", "packages/unselected"]);
    assert!(has_snapshot(&wanted, PARENT, "100.0.0"));
    assert!(has_snapshot(&wanted, DEP, "100.1.0"));
    let current = fixture.current();
    assert_eq!(importer_ids(&current), BTreeSet::from(["packages/selected".to_string()]));
    assert!(!has_snapshot(&current, PARENT, "100.0.0"));
    assert!(!has_snapshot(&current, DEP, "100.1.0"));
    let state = fixture.state();
    assert_eq!(
        state.projects.keys().cloned().collect::<BTreeSet<_>>(),
        [selected, unselected].into_iter().map(canonical_path).collect(),
    );
    assert!(state.filtered_install);
}

#[test]
fn filtered_fresh_install_materializes_only_selected_isolated_closure() {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["--filter", "selected", "install"]);
    assert_selected_isolated_closure(&fixture, &selected, &unselected);
}

#[test]
fn filtered_frozen_install_materializes_only_selected_isolated_closure() {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    fixture.run(["--filter", "selected", "install", "--frozen-lockfile"]);
    assert_selected_isolated_closure(&fixture, &selected, &unselected);
}

#[test]
fn unfiltered_install_after_filtered_install_restores_all_projects() {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["--filter", "selected", "install"]);
    assert_selected_isolated_closure(&fixture, &selected, &unselected);

    fixture.run(["install"]);
    assert!(has_link(&selected, HELLO));
    assert!(has_link(&unselected, PARENT));
    assert!(fixture.slot(PARENT, "100.0.0").exists());
    assert!(fixture.slot(DEP, "100.1.0").exists());
    assert_full_wanted(&fixture.current(), &["packages/selected", "packages/unselected"]);
    assert!(!fixture.state().filtered_install);
}

#[test]
fn filtered_frozen_install_checks_only_selected_manifest_specifiers() {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    let before = fixture.wanted();
    let prior_importer = importer(&before, "packages/unselected").clone();
    let prior_parent = snapshot_entries(&before, PARENT);
    let prior_child = snapshot_entries(&before, DEP);
    replace_dependencies(&unselected, &[(HELLO_PARENT, "1.0.0")]);
    let external_manifest = fs::read(unselected.join("package.json")).expect("read manifest");

    fixture.run(["--filter", "selected", "install", "--frozen-lockfile"]);
    let after = fixture.wanted();
    assert!(has_link(&selected, HELLO));
    assert!(!unselected.join("node_modules").exists());
    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        external_manifest,
    );
    assert_eq!(importer(&after, "packages/unselected"), &prior_importer);
    assert_eq!(snapshot_entries(&after, PARENT), prior_parent);
    assert_eq!(snapshot_entries(&after, DEP), prior_child);

    replace_dependencies(&selected, &[(NO_DEPS, "1.0.0")]);
    let output = fixture
        .command_at(&fixture.workspace, ["--filter", "selected", "install", "--frozen-lockfile"]);
    assert!(!output.status.success(), "selected manifest mismatch must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pacquet_package_manager::outdated_lockfile")
            && stderr.contains("Cannot install with \"frozen-lockfile\"")
            && stderr.contains("pnpm-lock.yaml is not up"),
        "expected the existing frozen-lockfile mismatch diagnostic:\n{stderr}",
    );
}

fn map_contains(value: &Value, needle: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.iter().any(|(key, value)| key.contains(needle) || map_contains(value, needle))
        }
        Value::Array(array) => array.iter().any(|value| map_contains(value, needle)),
        Value::String(value) => value.contains(needle),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[test]
fn filtered_install_after_full_install_preserves_unselected_materialization() {
    let fixture = FilteredWorkspace::new();
    fixture.append_workspace_yaml(
        "nodeExperimentalPackageMap: true\nhoistPattern:\n  - '*'\nmodulesCacheMaxAge: 0\n",
    );
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install"]);
    let before_wanted = fixture.wanted();
    let before_current = fixture.current();
    let prior_wanted_importer = importer(&before_wanted, "packages/unselected").clone();
    let prior_current_importer = importer(&before_current, "packages/unselected").clone();
    let prior_parent = snapshot_entries(&before_wanted, PARENT);
    let prior_child = snapshot_entries(&before_wanted, DEP);
    let prior_current_parent = snapshot_entries(&before_current, PARENT);
    let prior_current_child = snapshot_entries(&before_current, DEP);
    let prior_package_map = fixture.package_map();
    assert!(map_contains(&prior_package_map, PARENT));
    assert!(map_contains(&prior_package_map, DEP));
    let mut modules = fixture.modules();
    let prior_hoisted: HashMap<_, _> = modules
        .hoisted_dependencies
        .iter()
        .filter(|(key, _)| key.contains(PARENT) || key.contains(DEP))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    assert!(!prior_hoisted.is_empty(), "unselected hoist metadata must be present");
    let pending_id = snapshot_entries(&before_wanted, PARENT)[0].0.clone();
    modules.pending_builds.push(pending_id.clone());
    fixture.write_modules(modules);
    let retained_link = unselected.join("node_modules").join(PARENT);
    let retained_parent_slot = fixture.slot(PARENT, "100.0.0");
    let retained_child_slot = fixture.slot(DEP, "100.1.0");
    let obsolete_selected_slot = fixture.slot(HELLO, "1.0.0");
    assert!(has_link(&unselected, PARENT));
    assert!(retained_parent_slot.exists());
    assert!(retained_child_slot.exists());
    assert!(obsolete_selected_slot.exists());

    replace_dependencies(&selected, &[(NO_DEPS, "1.0.0")]);
    replace_dependencies(&unselected, &[(HELLO_PARENT, "1.0.0")]);
    let external_manifest = fs::read(unselected.join("package.json")).expect("read manifest");
    fixture.run(["--filter", "selected", "install"]);
    let after_wanted = fixture.wanted();
    let after_current = fixture.current();

    assert_eq!(
        fs::read(unselected.join("package.json")).expect("read manifest"),
        external_manifest,
    );
    assert_eq!(importer(&after_wanted, "packages/unselected"), &prior_wanted_importer);
    assert_eq!(snapshot_entries(&after_wanted, PARENT), prior_parent);
    assert_eq!(snapshot_entries(&after_wanted, DEP), prior_child);
    assert!(snapshot_entries(&after_wanted, HELLO_PARENT).is_empty());
    assert!(retained_link.exists());
    assert!(retained_parent_slot.exists());
    assert!(retained_child_slot.exists());
    assert_eq!(importer(&after_current, "packages/unselected"), &prior_current_importer);
    assert_eq!(snapshot_entries(&after_current, PARENT), prior_current_parent);
    assert_eq!(snapshot_entries(&after_current, DEP), prior_current_child);
    let after_package_map = fixture.package_map();
    assert!(map_contains(&after_package_map, PARENT));
    assert!(map_contains(&after_package_map, DEP));
    let after_modules = fixture.modules();
    for (key, value) in prior_hoisted {
        assert_eq!(after_modules.hoisted_dependencies.get(&key), Some(&value));
    }
    assert!(after_modules.pending_builds.contains(&pending_id));
    assert!(!obsolete_selected_slot.exists());
    assert!(!has_snapshot(&after_current, HELLO, "1.0.0"));
    assert!(has_link(&selected, NO_DEPS));
    assert!(fixture.slot(NO_DEPS, "1.0.0").exists());
    assert_eq!(importer_version(&after_wanted, "packages/selected", NO_DEPS), "1.0.0");
    assert_eq!(importer_version(&after_current, "packages/selected", NO_DEPS), "1.0.0");
}

fn importer_has_group_dependency(
    lockfile: &Lockfile,
    id: &str,
    group: &str,
    dependency: &str,
) -> bool {
    let name: PkgName = dependency.parse().expect("parse package name");
    let importer = importer(lockfile, id);
    match group {
        "dependencies" => importer.dependencies.as_ref(),
        "devDependencies" => importer.dev_dependencies.as_ref(),
        "optionalDependencies" => importer.optional_dependencies.as_ref(),
        _ => panic!("unsupported dependency group {group}"),
    }
    .is_some_and(|dependencies| dependencies.contains_key(&name))
}

#[test]
fn filtered_prod_install_prunes_direct_links_only_in_selected_projects() {
    let fixture = FilteredWorkspace::new();
    let deps = || ManifestDeps {
        prod: &[(HELLO, "1.0.0")],
        dev: &[(NO_DEPS, "1.0.0")],
        ..Default::default()
    };
    let selected = fixture.project("selected", "selected", deps());
    let unselected = fixture.project("unselected", "unselected", deps());
    fixture.run(["install"]);
    assert!(has_link(&selected, NO_DEPS));
    assert!(has_link(&unselected, NO_DEPS));

    fixture.run(["--filter", "selected", "install", "--prod"]);
    let current = fixture.current();
    assert!(!has_link(&selected, NO_DEPS));
    assert!(has_link(&unselected, NO_DEPS));
    assert!(!importer_has_group_dependency(
        &current,
        "packages/selected",
        "devDependencies",
        NO_DEPS,
    ));
    assert!(importer_has_group_dependency(
        &current,
        "packages/unselected",
        "devDependencies",
        NO_DEPS,
    ));
    assert!(has_snapshot(&current, NO_DEPS, "1.0.0"));
}

#[test]
fn sequential_filtered_prod_installs_prune_each_selected_project() {
    let fixture = FilteredWorkspace::new();
    let deps = || ManifestDeps {
        prod: &[(HELLO, "1.0.0")],
        dev: &[(NO_DEPS, "1.0.0")],
        ..Default::default()
    };
    let first = fixture.project("first", "first", deps());
    let second = fixture.project("second", "second", deps());
    fixture.run(["install"]);
    assert!(has_link(&first, NO_DEPS));
    assert!(has_link(&second, NO_DEPS));

    fixture.run(["--filter", "first", "install", "--prod"]);
    assert!(!has_link(&first, NO_DEPS));
    assert!(has_link(&second, NO_DEPS));

    fixture.run(["--filter", "second", "install", "--prod"]);
    assert!(!has_link(&second, NO_DEPS));
}

#[test]
fn filtered_prod_install_prunes_workspace_link_closure_projects() {
    let fixture = FilteredWorkspace::new();
    fixture.append_workspace_yaml("linkWorkspacePackages: true\n");
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[("linked", "workspace:*")], ..Default::default() },
    );
    let linked = fixture.project(
        "linked",
        "linked",
        ManifestDeps {
            prod: &[(HELLO, "1.0.0")],
            dev: &[(NO_DEPS, "1.0.0")],
            ..Default::default()
        },
    );
    fixture.run(["install"]);
    assert!(has_link(&selected, "linked"));
    assert!(has_link(&linked, NO_DEPS));

    fixture.run(["--filter", "selected", "install", "--prod"]);

    assert!(has_link(&selected, "linked"));
    assert!(!has_link(&linked, NO_DEPS));
}

#[test]
fn filtered_install_keeps_full_cleanup_for_shared_layout_drift() {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let _unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["install"]);
    let root_sentinel = fixture.workspace.join("node_modules/shared-layout-sentinel");
    let slot_sentinel = fixture.slot(PARENT, "100.0.0").join("layout-sentinel");
    fs::write(&root_sentinel, "stale").expect("write root sentinel");
    fs::write(&slot_sentinel, "stale").expect("write slot sentinel");
    let modules_path = fixture.workspace.join("node_modules/.modules.yaml");
    let mut raw_modules: Value =
        serde_json::from_str(&fs::read_to_string(&modules_path).expect("read .modules.yaml"))
            .expect("parse .modules.yaml as JSON");
    raw_modules["layoutVersion"] = json!(4);
    fs::write(
        &modules_path,
        serde_json::to_string_pretty(&raw_modules).expect("serialize incompatible modules"),
    )
    .expect("write incompatible .modules.yaml");

    fixture.run(["--filter", "selected", "install"]);
    assert!(!root_sentinel.exists());
    assert!(!slot_sentinel.exists());
    assert!(has_link(&selected, HELLO));
    let current = fixture.current();
    assert_eq!(importer_ids(&current), BTreeSet::from(["packages/selected".to_string()]));
    assert!(!has_snapshot(&current, PARENT, "100.0.0"));
    let modules = fixture.modules();
    assert!(!modules.pending_builds.iter().any(|entry| entry.contains(PARENT)));
    assert!(!modules.hoisted_dependencies.keys().any(|entry| entry.contains(PARENT)));
}

#[test]
fn filtered_hoisted_install_materializes_full_shared_graph_but_links_only_selected_projects() {
    let fixture = FilteredWorkspace::new();
    fixture.append_workspace_yaml("nodeLinker: hoisted\nnodeExperimentalPackageMap: true\n");
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["--filter", "selected", "install"]);

    for dependency in [HELLO, PARENT, DEP] {
        assert!(
            fixture.workspace.join("node_modules").join(dependency).is_dir(),
            "hoisted shared graph is missing {dependency}",
        );
    }
    assert!(selected.join("node_modules").join(HELLO).exists());
    assert!(!unselected.join("node_modules").exists());
    assert_full_wanted(&fixture.wanted(), &["packages/selected", "packages/unselected"]);
    let current = fixture.current();
    assert_full_wanted(&current, &["packages/selected", "packages/unselected"]);
    assert!(has_snapshot(&current, PARENT, "100.0.0"));
    assert!(has_snapshot(&current, DEP, "100.1.0"));
    let package_map = fixture.package_map();
    assert_eq!(
        package_map["packages"]["../packages/unselected"]["dependencies"]["unselected"],
        json!("../packages/unselected"),
    );
}

#[test]
fn filtered_isolated_install_expands_workspace_link_closure() {
    let fixture = FilteredWorkspace::new();
    fixture.append_workspace_yaml("linkWorkspacePackages: deep\n");
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[("second", "workspace:*")], ..Default::default() },
    );
    let second = fixture.project(
        "second",
        "second",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let third = fixture.project(
        "third",
        DEP,
        ManifestDeps { prod: &[("selected", "workspace:*")], ..Default::default() },
    );
    set_version(&third, "100.1.0");
    let unrelated = fixture.project(
        "unrelated",
        "unrelated",
        ManifestDeps { prod: &[(NO_DEPS, "1.0.0")], ..Default::default() },
    );
    fixture.run(["--filter", "selected", "install"]);

    assert!(has_link(&selected, "second"));
    assert!(has_link(&second, PARENT));
    assert!(has_link(&third, "selected"));
    assert!(fixture.slot(PARENT, "100.0.0").exists());
    assert!(!unrelated.join("node_modules").exists());
    assert!(!fixture.slot(NO_DEPS, "1.0.0").exists());
    assert_full_wanted(
        &fixture.wanted(),
        &["packages/second", "packages/selected", "packages/third", "packages/unrelated"],
    );
}

#[test]
fn filtered_pnp_install_uses_isolated_placeholder_scope() {
    let fixture = FilteredWorkspace::new();
    fixture.append_workspace_yaml("nodeLinker: pnp\n");
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["--filter", "selected", "install"]);

    assert!(has_link(&selected, HELLO));
    assert!(fixture.slot(HELLO, "1.0.0").exists());
    assert!(!unselected.join("node_modules").exists());
    assert!(!fixture.slot(PARENT, "100.0.0").exists());
    assert!(!fixture.slot(DEP, "100.1.0").exists());
    assert_full_wanted(&fixture.wanted(), &["packages/selected", "packages/unselected"]);
    assert!(!fixture.workspace.join("node_modules/.package-map.json").exists());
}

#[test]
fn install_selection_uses_post_update_config_workspace_graph() {
    let fixture = FilteredWorkspace::new();
    let app = fixture.project(
        "app",
        "app",
        ManifestDeps { prod: &[("lib", "1.0.0")], ..Default::default() },
    );
    let lib = fixture.project("lib", "lib", ManifestDeps::default());
    fs::write(
        fixture.workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.linkWorkspacePackages = true; return config } } }\n",
    )
    .expect("write updateConfig hook");

    fixture.run(["--filter", "app...", "add", HELLO, "--lockfile-only"]);
    let wanted = fixture.wanted();
    assert_eq!(dependency_spec(&app, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(dependency_spec(&lib, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(importer_version(&wanted, "packages/app", "lib"), "link:../lib");
    assert_eq!(importer_version(&wanted, "packages/lib", HELLO), "1.0.0");
}

fn recursive_add_prefixes(no_sort: bool) -> (Vec<String>, String, String) {
    let fixture = FilteredWorkspace::new();
    let app = fixture.project(
        "app",
        "app",
        ManifestDeps { prod: &[("lib", "workspace:*")], ..Default::default() },
    );
    let lib = fixture.project("lib", "lib", ManifestDeps::default());
    let args = if no_sort {
        vec!["--no-sort", "--filter", "app...", "add", HELLO, "--lockfile-only"]
    } else {
        vec!["--filter", "app...", "add", HELLO, "--lockfile-only"]
    };
    let prefixes = initial_manifest_prefixes(&fixture.run(args));
    (prefixes, canonical_path(&app), canonical_path(&lib))
}

#[test]
fn recursive_no_sort_preserves_selector_discovery_order() {
    let (sorted, app, lib) = recursive_add_prefixes(false);
    assert_eq!(sorted, vec![lib, app]);
    let (unsorted, app, lib) = recursive_add_prefixes(true);
    assert_eq!(unsorted, vec![app, lib]);
}

#[test]
fn workspace_without_root_manifest_does_not_create_root_importer() {
    let fixture = FilteredWorkspace::new();
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["--filter", "selected", "install"]);

    assert!(!fixture.workspace.join("package.json").exists());
    let wanted = fixture.wanted();
    assert_full_wanted(&wanted, &["packages/selected", "packages/unselected"]);
    assert!(!wanted.importers.contains_key("."));
    let state = fixture.state();
    assert_eq!(
        state.projects.keys().cloned().collect::<BTreeSet<_>>(),
        [selected, unselected].into_iter().map(|path| canonical_path(&path)).collect(),
    );
    assert!(!state.projects.contains_key(&canonical_path(&fixture.workspace)));
}

#[test]
fn workspace_non_project_subdirectory_does_not_create_active_importer() {
    let fixture = FilteredWorkspace::new();
    fixture.write_root_manifest("workspace-root", ManifestDeps::default());
    let selected = fixture.project(
        "selected",
        "selected",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let unselected = fixture.project(
        "unselected",
        "unselected",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let scratch = fixture.workspace.join("tools/scratch");
    fs::create_dir_all(&scratch).expect("create non-project subdirectory");
    fixture.run_at(&scratch, ["--filter", "selected", "install"]);

    assert!(!scratch.join("package.json").exists());
    let wanted = fixture.wanted();
    assert_full_wanted(&wanted, &[".", "packages/selected", "packages/unselected"]);
    assert!(!wanted.importers.contains_key("tools/scratch"));
    let state = fixture.state();
    assert_eq!(
        state.projects.keys().cloned().collect::<BTreeSet<_>>(),
        [&fixture.workspace, &selected, &unselected]
            .into_iter()
            .map(|path| canonical_path(path))
            .collect(),
    );
    assert!(!state.projects.contains_key(&canonical_path(&scratch)));
}

#[test]
fn filtered_install_rejects_per_project_workspace_lockfiles() {
    let fixture = FilteredWorkspace::new();
    fixture.append_workspace_yaml("sharedWorkspaceLockfile: false\n");
    fixture.project("app", "app", ManifestDeps::default());

    let output =
        fixture.command_at(&fixture.workspace, ["--filter", "app", "install", "--lockfile-only"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_SHARED_LOCKFILE_UNSUPPORTED")
            && stderr.contains("sharedWorkspaceLockfile=false"),
        "stderr: {stderr}",
    );
    assert!(!fixture.workspace.join("pnpm-lock.yaml").exists());
}

#[test]
fn recursive_add_auto_excludes_workspace_root() {
    let fixture = FilteredWorkspace::new();
    fixture.write_root_manifest("workspace-root", ManifestDeps::default());
    let member_a = fixture.project("member-a", "member-a", ManifestDeps::default());
    let member_b = fixture.project("member-b", "member-b", ManifestDeps::default());
    fixture.run(["-r", "add", HELLO, "--lockfile-only"]);
    let wanted = fixture.wanted();

    assert_eq!(dependency_spec(&fixture.workspace, "dependencies", HELLO), None);
    assert_eq!(dependency_spec(&member_a, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert_eq!(dependency_spec(&member_b, "dependencies", HELLO).as_deref(), Some("^1.0.0"));
    assert!(!importer_has_group_dependency(&wanted, ".", "dependencies", HELLO));
    assert!(importer_has_group_dependency(&wanted, "packages/member-a", "dependencies", HELLO,));
    assert!(importer_has_group_dependency(&wanted, "packages/member-b", "dependencies", HELLO,));
}

#[test]
fn filter_matching_every_real_project_is_not_partial() {
    let fixture = FilteredWorkspace::new();
    let member_a = fixture.project(
        "member-a",
        "member-a",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let member_b = fixture.project(
        "member-b",
        "member-b",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    fixture.run(["--filter", "*", "install"]);
    let state = fixture.state();

    assert!(!state.filtered_install);
    assert_eq!(
        state.projects.keys().cloned().collect::<BTreeSet<_>>(),
        [&member_a, &member_b].into_iter().map(|path| canonical_path(path)).collect(),
    );
    assert_full_wanted(&fixture.wanted(), &["packages/member-a", "packages/member-b"]);
    assert_full_wanted(&fixture.current(), &["packages/member-a", "packages/member-b"]);
    assert!(has_link(&member_a, HELLO));
    assert!(has_link(&member_b, PARENT));
}

#[test]
fn filtered_install_refreshes_unselected_catalog_importers_when_catalog_changes() {
    let fixture = FilteredWorkspace::new();
    fixture.append_workspace_yaml(&format!("catalog:\n  '{CATALOG_FOO}': 1.0.0\n"));
    fixture.project("app", "app", ManifestDeps::default());
    fixture.project(
        "catalog-consumer",
        "catalog-consumer",
        ManifestDeps { prod: &[(CATALOG_FOO, "catalog:")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    assert_eq!(
        importer_version(&fixture.wanted(), "packages/catalog-consumer", CATALOG_FOO),
        "1.0.0",
    );

    let workspace_yaml_path = fixture.workspace.join("pnpm-workspace.yaml");
    let workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read workspace yaml");
    fs::write(
        &workspace_yaml_path,
        workspace_yaml
            .replace(&format!("'{CATALOG_FOO}': 1.0.0"), &format!("'{CATALOG_FOO}': 2.0.0")),
    )
    .expect("update catalog");

    fixture.run(["--filter", "app", "install", "--lockfile-only"]);
    let wanted = fixture.wanted();
    assert_eq!(importer_version(&wanted, "packages/catalog-consumer", CATALOG_FOO), "2.0.0",);
    let catalog = wanted
        .catalogs
        .as_ref()
        .and_then(|catalogs| catalogs.get("default"))
        .and_then(|entries| entries.get(CATALOG_FOO))
        .expect("catalog snapshot");
    assert_eq!(catalog.specifier, "2.0.0");
    assert_eq!(catalog.version, "2.0.0");
}

#[test]
fn active_manifest_outside_workspace_patterns_keeps_install_filtered() {
    let fixture = FilteredWorkspace::new();
    let member = fixture.project(
        "member",
        "member",
        ManifestDeps { prod: &[(HELLO, "1.0.0")], ..Default::default() },
    );
    let local = fixture.workspace.join("tools/local");
    write_manifest(
        &local,
        "local",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run_at(&local, ["--filter", "*", "install"]);

    let state = fixture.state();
    assert!(state.filtered_install);
    assert_eq!(
        state.projects.keys().cloned().collect::<BTreeSet<_>>(),
        [&member, &local].into_iter().map(|path| canonical_path(path)).collect(),
    );
    assert!(has_link(&member, HELLO));
    assert!(!local.join("node_modules").exists());
}
