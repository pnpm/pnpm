use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{Lockfile, PkgName, ProjectSnapshot, SnapshotEntry};
use pacquet_modules_yaml::{Host as ModulesHost, Modules, read_modules_manifest};
use pacquet_store_dir::{CafsFileInfo, StoreDir, StoreIndex};
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use pacquet_workspace_state::WorkspaceState;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

/// Flip the `enableGlobalVirtualStore` key in the `pnpm-workspace.yaml`
/// that [`pacquet_testing_utils::bin::CommandTempCwd::add_mocked_registry`]
/// populated with `storeDir` / `cacheDir` / `enableGlobalVirtualStore: false`.
/// The replacement is in-place rather than appended so the file stays
/// valid YAML (pnpm rejects duplicate top-level mapping keys).
pub fn enable_gvs_in_workspace_yaml(workspace: &Path, extra_yaml: &str) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let flipped = yaml.replace("enableGlobalVirtualStore: false", "enableGlobalVirtualStore: true");
    assert_ne!(
        flipped, yaml,
        "expected the default `enableGlobalVirtualStore: false` line written by \
         `CommandTempCwd::add_mocked_registry` — has the helper changed?",
    );
    let mut yaml = flipped;
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(extra_yaml);
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// Minutes elapsed since 2022-03-01T00:00:00Z, for [`set_minimum_release_age`].
///
/// The mocked registry publishes `@pnpm.e2e/bravo-dep` at 1.0.0
/// (2022-02-01), 1.0.1 (2022-02-22), and 1.1.0 (2022-05-01, the `latest`
/// tag) — see `version_publish_time` in `pnpr/crates/pnpr-fixtures/src/lib.rs`
/// — so this cutoff makes 1.1.0 the only immature version.
#[must_use]
pub fn bravo_dep_mature_up_to_1_0_1_minimum_release_age() -> u64 {
    const CUTOFF_UNIX_SECS: u64 = 1_646_092_800; // 2022-03-01T00:00:00Z
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs();
    (now_secs - CUTOFF_UNIX_SECS) / 60
}

/// Append a top-level `key: value` line to the `pnpm-workspace.yaml` the
/// harness already wrote. Appending is only valid while the harness never
/// writes the key itself (pnpm rejects duplicate top-level mapping keys),
/// so the guard assert fails loudly if that changes.
pub fn append_workspace_yaml_key(workspace: &Path, key: &str, value: impl std::fmt::Display) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let key_prefix = format!("{key}:");
    assert!(
        !yaml.lines().any(|line| line.starts_with(&key_prefix)),
        "pnpm-workspace.yaml already has a `{key}:` key — update this helper",
    );
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    writeln!(yaml, "{key}: {value}").unwrap();
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// [`append_workspace_yaml_key`] for the `minimumReleaseAge` setting.
pub fn set_minimum_release_age(workspace: &Path, minutes: u64) {
    append_workspace_yaml_key(workspace, "minimumReleaseAge", minutes);
}

/// Snapshot-friendly view of every row in `<store>/v11/index.db`.
///
/// The outer key is the `SQLite` key (`"{integrity}\t{pkgId}"`). The inner
/// map is the package's files — one entry per path inside the tarball.
/// `checked_at` is scrubbed because its value depends on install time.
#[must_use]
pub fn index_file_contents(store_dir: &Path) -> BTreeMap<String, BTreeMap<String, CafsFileInfo>> {
    let store = StoreDir::new(store_dir);
    // open_readonly: we're just reading for snapshot assertions, so don't
    // create WAL sidecars or otherwise mutate the store.
    let index = StoreIndex::open_readonly_in(&store).expect("open v11 index.db");

    let mut out = BTreeMap::new();
    for key in index.keys().expect("list index keys") {
        let row = index.get(&key).expect("read index row").expect("row disappeared");
        let files = row
            .files
            .into_iter()
            .map(|(filename, mut info)| {
                info.checked_at = None;
                (filename, info)
            })
            .collect();
        out.insert(key, files);
    }
    out
}

/// Parse `<workspace>/node_modules/.pnpm/lock.yaml` — the current
/// lockfile describing what the last install materialized.
#[must_use]
pub fn read_current_lockfile(workspace: &Path) -> pacquet_lockfile::Lockfile {
    let text = fs::read_to_string(workspace.join("node_modules/.pnpm/lock.yaml"))
        .expect("read the current lockfile");
    serde_saphyr::from_str(&text).expect("parse the current lockfile")
}

/// Dependency groups for [`write_project_manifest`] /
/// [`WorkspaceFixture::project`].
#[derive(Default, Clone, Copy)]
pub struct ManifestDeps<'a> {
    pub prod: &'a [(&'a str, &'a str)],
    pub dev: &'a [(&'a str, &'a str)],
    pub optional: &'a [(&'a str, &'a str)],
    pub peer: &'a [(&'a str, &'a str)],
}

/// A `packages/*` workspace against the mocked registry: project
/// scaffolding, CLI invocation with NDJSON capture, and readers for
/// the lockfiles and install-state files.
pub struct WorkspaceFixture {
    /// RAII guard for the temporary directory the workspace lives in.
    _root: TempDir,
    pub workspace: PathBuf,
    pub registry: AddMockedRegistry,
}

impl WorkspaceFixture {
    #[must_use]
    pub fn new() -> Self {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let fixture = Self { _root: root, workspace, registry: npmrc_info };
        fixture.append_workspace_yaml("packages:\n  - 'packages/*'\n");
        fixture
    }

    pub fn append_workspace_yaml(&self, text: &str) {
        let path = self.workspace.join("pnpm-workspace.yaml");
        let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
        if !yaml.ends_with('\n') {
            yaml.push('\n');
        }
        yaml.push_str(text);
        fs::write(path, yaml).expect("write pnpm-workspace.yaml");
    }

    pub fn write_root_manifest(&self, name: &str, deps: ManifestDeps<'_>) {
        write_project_manifest(&self.workspace, name, deps);
    }

    #[expect(
        clippy::must_use_candidate,
        reason = "many callers scaffold a project for its side effect and never need the returned path"
    )]
    pub fn project(&self, dir: &str, name: &str, deps: ManifestDeps<'_>) -> PathBuf {
        let project = self.workspace.join("packages").join(dir);
        write_project_manifest(&project, name, deps);
        project
    }

    pub fn command_at<Args, Arg>(&self, cwd: &Path, args: Args) -> Output
    where
        Args: IntoIterator<Item = Arg>,
        Arg: AsRef<OsStr>,
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

    pub fn run<Args, Arg>(&self, args: Args) -> Vec<Value>
    where
        Args: IntoIterator<Item = Arg>,
        Arg: AsRef<OsStr>,
    {
        self.run_at(&self.workspace, args)
    }

    pub fn run_at<Args, Arg>(&self, cwd: &Path, args: Args) -> Vec<Value>
    where
        Args: IntoIterator<Item = Arg>,
        Arg: AsRef<OsStr>,
    {
        let output = self.command_at(cwd, args);
        assert_success(&output);
        ndjson_records(&output)
    }

    #[must_use]
    pub fn wanted(&self) -> Lockfile {
        read_lockfile(&self.workspace.join("pnpm-lock.yaml"))
    }

    #[must_use]
    pub fn current(&self) -> Lockfile {
        read_lockfile(&self.workspace.join("node_modules/.pnpm/lock.yaml"))
    }

    #[must_use]
    pub fn modules(&self) -> Modules {
        read_modules_manifest::<ModulesHost>(&self.workspace.join("node_modules"))
            .expect("read .modules.yaml")
            .expect(".modules.yaml exists")
    }

    pub fn write_modules(&self, modules: Modules) {
        pacquet_modules_yaml::write_modules_manifest::<ModulesHost>(
            &self.workspace.join("node_modules"),
            modules,
        )
        .expect("write .modules.yaml");
    }

    #[must_use]
    pub fn state(&self) -> WorkspaceState {
        let path = self.workspace.join("node_modules/.pnpm-workspace-state-v1.json");
        serde_json::from_str(&fs::read_to_string(path).expect("read workspace state"))
            .expect("parse workspace state")
    }

    #[must_use]
    pub fn package_map(&self) -> Value {
        let path = self.workspace.join("node_modules/.package-map.json");
        serde_json::from_str(&fs::read_to_string(path).expect("read package map"))
            .expect("parse package map")
    }

    #[must_use]
    pub fn slot(&self, name: &str, version: &str) -> PathBuf {
        self.workspace
            .join("node_modules/.pnpm")
            .join(format!("{}@{version}", name.replace('/', "+")))
    }
}

impl Default for WorkspaceFixture {
    fn default() -> Self {
        Self::new()
    }
}

pub fn write_project_manifest(project: &Path, name: &str, deps: ManifestDeps<'_>) {
    fs::create_dir_all(project).expect("create project directory");
    let mut manifest = Map::from_iter([
        ("name".to_string(), Value::String(name.to_string())),
        ("version".to_string(), Value::String("1.0.0".to_string())),
        ("private".to_string(), Value::Bool(true)),
    ]);
    insert_dependency_group(&mut manifest, "dependencies", deps.prod);
    insert_dependency_group(&mut manifest, "devDependencies", deps.dev);
    insert_dependency_group(&mut manifest, "optionalDependencies", deps.optional);
    insert_dependency_group(&mut manifest, "peerDependencies", deps.peer);
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

#[must_use]
pub fn read_manifest(project: &Path) -> Value {
    serde_json::from_str(
        &fs::read_to_string(project.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json")
}

pub fn write_manifest_value(project: &Path, manifest: &Value) {
    fs::write(
        project.join("package.json"),
        serde_json::to_string_pretty(manifest).expect("serialize package.json"),
    )
    .expect("write package.json");
}

pub fn set_dependency(project: &Path, group: &str, name: &str, spec: &str) {
    let mut manifest = read_manifest(project);
    let object = manifest.as_object_mut().expect("manifest is an object");
    let dependencies = object.entry(group).or_insert_with(|| json!({}));
    dependencies
        .as_object_mut()
        .expect("dependency group is an object")
        .insert(name.to_string(), Value::String(spec.to_string()));
    write_manifest_value(project, &manifest);
}

pub fn set_version(project: &Path, version: &str) {
    let mut manifest = read_manifest(project);
    manifest["version"] = Value::String(version.to_string());
    write_manifest_value(project, &manifest);
}

pub fn replace_dependencies(project: &Path, deps: &[(&str, &str)]) {
    let mut manifest = read_manifest(project);
    manifest["dependencies"] = Value::Object(
        deps.iter()
            .map(|(name, spec)| (name.to_string(), Value::String(spec.to_string())))
            .collect(),
    );
    write_manifest_value(project, &manifest);
}

#[must_use]
pub fn dependency_spec(project: &Path, group: &str, name: &str) -> Option<String> {
    read_manifest(project)
        .get(group)
        .and_then(Value::as_object)
        .and_then(|dependencies| dependencies.get(name))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[must_use]
pub fn read_lockfile(path: &Path) -> Lockfile {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read lockfile {}: {error}", path.display()));
    serde_saphyr::from_str(&contents)
        .unwrap_or_else(|error| panic!("parse lockfile {}: {error}\n{contents}", path.display()))
}

pub fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[must_use]
pub fn ndjson_records(output: &Output) -> Vec<Value> {
    [&output.stderr[..], &output.stdout[..]]
        .into_iter()
        .flat_map(|stream| {
            String::from_utf8_lossy(stream).lines().map(str::to_string).collect::<Vec<_>>()
        })
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

/// The `name: "pnpm" / level: "info"` log pnpm's headless installer
/// emits when it is entered with an up-to-date lockfile.
#[must_use]
pub fn has_up_to_date_log(records: &[Value]) -> bool {
    records.iter().any(|record| {
        record.get("name").and_then(Value::as_str) == Some("pnpm")
            && record.get("level").and_then(Value::as_str) == Some("info")
            && record.get("message").and_then(Value::as_str)
                == Some("Lockfile is up to date, resolution step is skipped")
    })
}

#[must_use]
pub fn importer<'a>(lockfile: &'a Lockfile, id: &str) -> &'a ProjectSnapshot {
    lockfile
        .importers
        .get(id)
        .unwrap_or_else(|| panic!("missing importer {id:?}: {:?}", lockfile.importers.keys()))
}

#[must_use]
pub fn importer_version(lockfile: &Lockfile, id: &str, name: &str) -> String {
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

#[must_use]
pub fn importer_specifier(lockfile: &Lockfile, id: &str, name: &str) -> String {
    let name: PkgName = name.parse().expect("parse package name");
    importer(lockfile, id)
        .dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(&name))
        .unwrap_or_else(|| panic!("missing dependency {name} in importer {id}"))
        .specifier
        .clone()
}

#[must_use]
pub fn importer_ids(lockfile: &Lockfile) -> BTreeSet<String> {
    lockfile.importers.keys().cloned().collect()
}

#[must_use]
pub fn snapshot_entries(lockfile: &Lockfile, name: &str) -> Vec<(String, SnapshotEntry)> {
    lockfile
        .snapshots
        .as_ref()
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.to_string().starts_with(&format!("{name}@")))
        .map(|(key, snapshot)| (key.to_string(), snapshot.clone()))
        .collect()
}

#[must_use]
pub fn has_snapshot(lockfile: &Lockfile, name: &str, version: &str) -> bool {
    lockfile.snapshots.as_ref().is_some_and(|snapshots| {
        snapshots.keys().any(|key| {
            let key = key.to_string();
            key == format!("{name}@{version}") || key.starts_with(&format!("{name}@{version}("))
        })
    })
}

#[must_use]
pub fn has_link(project: &Path, name: &str) -> bool {
    is_symlink_or_junction(&project.join("node_modules").join(name)).unwrap_or(false)
}

#[must_use]
pub fn canonical_path(path: &Path) -> String {
    dunce::canonicalize(path)
        .unwrap_or_else(|error| panic!("canonicalize {}: {error}", path.display()))
        .to_string_lossy()
        .into_owned()
}

pub fn assert_full_wanted(lockfile: &Lockfile, ids: &[&str]) {
    assert_eq!(
        importer_ids(lockfile),
        ids.iter().map(ToString::to_string).collect(),
        "wanted lockfile must retain every real importer",
    );
}

#[must_use]
pub fn importer_has_group_dependency(
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
