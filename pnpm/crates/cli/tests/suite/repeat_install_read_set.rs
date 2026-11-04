//! The repeat-install fast path stands in for a full install, so every file
//! that install opens in the workspace outside `node_modules` is an input the
//! fast path has to see change. `node_modules` holds what the install writes,
//! which the fast path checks on its own. Each one must be a file the workspace state records, or be
//! classified here with the reason it needs no record. A file a new input
//! makes the install read fails the test until it is one or the other.
//!
//! Each file is of one [`Input`] kind, and every kind must be observed. A new
//! kind needs an edit scenario in `repeat_install_oracle` to compile.
//!
//! The install observed is the full one that runs from the state the previous
//! install left, the one the fast path replaces. inotify observes its opens:
//! no filesystem seam reaches every read the install makes.

use crate::{
    _utils::{inotify::record_opens, pacquet_in},
    repeat_install_oracle::Input,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pnpm_workspace_state::{WorkspaceState, load_workspace_state};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use strum::IntoEnumIterator as _;

const CONFIG: &str = "the config loader reads it on every command, and \
    `settings_classification` classifies each setting it holds";
const DISCOVERY: &str = "project discovery lists it, and the fast path compares the projects \
    it finds with the recorded ones";

/// Every path a full install opens that the workspace state does not record,
/// relative to the workspace, with its kind and the reason the fast path needs
/// no record of it.
const CLASSIFICATION: &[(&str, Input, &str)] = &[
    (".npmrc", Input::Npmrc, CONFIG),
    ("pnpm-workspace.yaml", Input::WorkspaceManifest, CONFIG),
    ("packages", Input::ProjectDirectories, DISCOVERY),
];

/// A workspace that exercises some of the inputs of an install. Its root
/// `package.json` is a private manifest unless `files` writes one.
struct Fixture {
    name: &'static str,
    /// Lines appended to `pnpm-workspace.yaml`.
    settings: &'static str,
    files: fn() -> Vec<(String, Vec<u8>)>,
}

const PROJECTS: &str = "packages:\n  - packages/*\n";

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "hooks, patches, catalogs, and workspace commands",
        settings: "packages:\n  - packages/*\n\
            catalog:\n  is-positive: 1.0.0\n\
            patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n",
        files: || {
            vec![
                manifest("packages/a", &json!({ "b": "workspace:*", "is-positive": "catalog:" })),
                command("packages/b"),
                text(
                    ".pnpmfile.cjs",
                    "module.exports = { hooks: { readPackage: (pkg) => pkg } }\n",
                ),
                text("patches/is-positive@1.0.0.patch", crate::repeat_install::IS_POSITIVE_PATCH),
            ]
            .into_iter()
            .flatten()
            .collect()
        },
    },
    Fixture {
        name: "a local directory dependency",
        settings: PROJECTS,
        files: || {
            vec![
                manifest("packages/a", &json!({ "local": "file:../../local" })),
                manifest("local", &json!({})),
                text("local/index.js", "module.exports = 1\n"),
            ]
            .into_iter()
            .flatten()
            .collect()
        },
    },
    Fixture {
        name: "a local tarball dependency",
        settings: PROJECTS,
        files: || {
            let tarball = pnpm_testing_utils::fixtures::tarball_with_manifest(
                &json!({ "name": "packed", "version": "1.0.0" }),
            );
            vec![
                manifest("packages/a", &json!({ "packed": "file:../../packed-1.0.0.tgz" })),
                vec![("packed-1.0.0.tgz".to_string(), tarball)],
            ]
            .into_iter()
            .flatten()
            .collect()
        },
    },
    Fixture {
        name: "injected workspace packages",
        settings: "packages:\n  - packages/*\ninjectWorkspacePackages: true\n",
        files: || {
            vec![manifest("packages/a", &json!({ "b": "workspace:*" })), command("packages/b")]
                .into_iter()
                .flatten()
                .collect()
        },
    },
    Fixture {
        name: "project lockfiles",
        settings: "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
        files: || {
            vec![
                manifest("packages/a", &json!({ "b": "workspace:*", "is-positive": "1.0.0" })),
                manifest("packages/b", &json!({})),
            ]
            .into_iter()
            .flatten()
            .collect()
        },
    },
    Fixture {
        name: "a root project alone",
        settings: "",
        files: || manifest(".", &json!({ "is-positive": "1.0.0" })),
    },
];

fn text(path: &str, text: &str) -> Vec<(String, Vec<u8>)> {
    vec![(path.to_string(), text.as_bytes().to_vec())]
}

/// The `package.json` of the project in `dir`, named after the directory.
fn manifest(dir: &str, dependencies: &Value) -> Vec<(String, Vec<u8>)> {
    let name = dir
        .rsplit('/')
        .next()
        .filter(|name| *name != ".")
        .unwrap_or("root");
    let manifest = json!({ "name": name, "version": "1.0.0", "dependencies": dependencies });
    vec![(format!("{dir}/package.json"), manifest.to_string().into_bytes())]
}

/// A project in `dir` that declares the command `cli`.
fn command(dir: &str) -> Vec<(String, Vec<u8>)> {
    let manifest = json!({ "name": "b", "version": "1.0.0", "bin": { "cli": "cli.js" } });
    vec![
        (format!("{dir}/package.json"), manifest.to_string().into_bytes()),
        (format!("{dir}/cli.js"), b"#!/usr/bin/env node\n".to_vec()),
    ]
}

fn write_fixture(workspace: &Path, fixture: &Fixture) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str(fixture.settings);
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    let root_manifest = json!({ "name": "root", "private": true }).to_string().into_bytes();
    let root_manifest = ("package.json".to_string(), root_manifest);
    for (path, contents) in std::iter::once(root_manifest).chain((fixture.files)()) {
        let path = workspace.join(path);
        fs::create_dir_all(path.parent().expect("a file in a directory"))
            .expect("create the directory");
        fs::write(&path, contents).expect("write a fixture file");
    }
}

/// Every directory under `dir` outside `node_modules`, `dir` included.
fn source_dirs(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "node_modules")
        .map(|entry| entry.expect("walk the workspace"))
        .filter(|entry| entry.file_type().is_dir())
        .map(walkdir::DirEntry::into_path)
        .collect()
}

/// The files the workspace states record, and the local directory
/// dependencies they record. A workspace whose projects keep their own
/// lockfiles has a state in each project.
#[derive(Default)]
struct Records {
    files: BTreeMap<PathBuf, Input>,
    /// The recorded local directories that are not workspace projects. The
    /// fast path counts such a directory as changed, so a file in it needs no
    /// record of its own.
    blocking_directories: Vec<PathBuf>,
}

impl Records {
    fn of(workspace: &Path) -> Self {
        let mut records = Records::default();
        for root in source_dirs(workspace) {
            if let Some(state) = load_workspace_state(&root).expect("read a workspace state") {
                records.add(&root, state);
            }
        }
        assert!(!records.files.is_empty(), "the install wrote no workspace state");
        records
    }

    fn add(&mut self, root: &Path, state: WorkspaceState) {
        let validation = state.validation.expect("the install recorded a validation");
        let manifests = validation.manifest_files
            .iter()
            .map(|(importer, file)| root.join(importer).join(&file.file_name));
        let local = validation.local_dependencies.values();
        let projects = &state.projects;
        let tarballs: BTreeSet<&String> = local
            .clone()
            .flat_map(|deps| &deps.tarballs)
            .collect();
        let shared_files = validation.files
            .keys()
            .filter(|path| !state.pnpmfiles.contains(path) && !tarballs.contains(path))
            .map(|path| (PathBuf::from(path), shared_file_input(Path::new(path))));
        self.files.extend(
            shared_files
                .chain(
                    state.pnpmfiles
                        .iter()
                        .map(|path| (path.into(), Input::Pnpmfile)),
                )
                .chain(manifests.map(|path| (path, Input::Manifest)))
                .chain(
                    tarballs
                        .iter()
                        .map(|path| (path.into(), Input::LocalTarball)),
                ),
        );
        self.blocking_directories.extend(
            local
                .flat_map(|deps| &deps.directories)
                .filter(|dir| !projects.contains_key(*dir))
                .map(PathBuf::from),
        );
    }

    /// The kind of the recorded file at `path`, if the states record it.
    fn input_of(&self, path: &Path) -> Option<Input> {
        self.files
            .get(path)
            .copied()
            .or_else(|| {
                self.blocking_directories
                    .iter()
                    .any(|dir| path.starts_with(dir))
                    .then_some(Input::LocalDirectory)
            })
    }
}

/// The kind of a file `validation.files` records besides the pnpmfiles and
/// the local tarballs.
fn shared_file_input(path: &Path) -> Input {
    if path
        .file_name()
        .is_some_and(|name| name == "pnpm-lock.yaml")
    {
        Input::Lockfile
    } else if path
        .extension()
        .is_some_and(|extension| extension == "patch")
    {
        Input::PatchFile
    } else {
        panic!("a recorded file of no known kind: {}", path.display())
    }
}

/// What a full install of a fixture opens: the kind of each recorded file,
/// and the workspace-relative paths of the files the states do not record.
#[derive(Default)]
struct Reads {
    recorded: BTreeSet<Input>,
    unrecorded: BTreeSet<String>,
}

fn reads(fixture: &Fixture) -> Reads {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_fixture(&workspace, fixture);
    let install = |extra_env: &[(&str, &str)]| {
        let mut command =
            pacquet_in(&workspace).with_env("PNPM_CONFIG_REGISTRY", mock_instance.url());
        for (key, value) in extra_env {
            command = command.with_env(key, value);
        }
        command
            .with_arg("install")
            .assert()
            .success();
    };
    install(&[]);
    let records = Records::of(&workspace);
    let opens = record_opens(&source_dirs(&workspace), || {
        install(&[("PNPM_CONFIG_OPTIMISTIC_REPEAT_INSTALL", "false")]);
    });
    let mut reads = Reads::default();
    for path in opens {
        if let Some(input) = records.input_of(&path) {
            reads.recorded.insert(input);
        } else {
            let relative =
                path.strip_prefix(&workspace).expect("a watched path is in the workspace");
            reads.unrecorded.insert(relative.to_string_lossy().into_owned());
        }
    }
    drop((root, mock_instance));
    reads
}

#[test]
fn every_file_a_full_install_opens_is_recorded_or_classified() {
    let classified: BTreeMap<&str, Input> = CLASSIFICATION
        .iter()
        .map(|(path, input, _)| (*path, *input))
        .collect();
    let mut seen = BTreeSet::new();
    let mut observed = BTreeSet::new();
    let mut unclassified = Vec::new();
    for fixture in FIXTURES {
        let reads = reads(fixture);
        observed.extend(reads.recorded);
        for path in reads.unrecorded {
            match classified.get(path.as_str()) {
                Some(input) => {
                    observed.insert(*input);
                }
                None => unclassified.push(format!("{path} ({})", fixture.name)),
            }
            seen.insert(path);
        }
    }
    let unused: Vec<_> = classified
        .keys()
        .filter(|path| !seen.contains(**path))
        .collect();
    let unobserved: Vec<_> = Input::iter()
        .filter(|input| !observed.contains(input))
        .collect();
    assert!(
        unclassified.is_empty(),
        "a full install opens files the workspace state does not record: {unclassified:#?}",
    );
    assert!(unused.is_empty(), "classified files no fixture opens: {unused:?}");
    assert!(unobserved.is_empty(), "inputs no fixture reads: {unobserved:?}");
}
