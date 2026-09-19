use std::{collections::BTreeMap, fs, path::Path, time::SystemTime};

use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::{IncludedDependencies, LayoutVersion, ModulesLayout};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::{ProjectEntry, WorkspaceState, load_workspace_state};
use tempfile::tempdir;

use crate::install::{
    moved_tree_is_reusable,
    prepare_modules_state::{
        purge::{is_safe_modules_purge_target, purge_modules_dir_entries},
        recorded_workspace,
        up_to_date::{FrozenTreeUpToDate, frozen_tree_up_to_date},
    },
    state_options::{ModulesTreeContext, RecordedWorkspace, RepeatInstallPolicy},
};

#[test]
fn modules_purge_target_must_be_a_strict_workspace_descendant() {
    let workspace_root = Path::new("/workspace");
    let modules_dir = Path::new("/workspace/node_modules");

    assert!(!is_safe_modules_purge_target(workspace_root, workspace_root));
    assert!(is_safe_modules_purge_target(modules_dir, workspace_root));
    assert!(!is_safe_modules_purge_target(
        Path::new("/workspace-sibling/node_modules"),
        workspace_root,
    ));
}

#[test]
fn purge_removes_directory_links_without_following_them() {
    let dir = tempdir().unwrap();
    let modules_dir = dir.path().join("node_modules");
    let link_target = dir.path().join("link-target");
    fs::create_dir_all(modules_dir.join("plain-dir")).unwrap();
    fs::write(modules_dir.join("plain-file"), "").unwrap();
    fs::create_dir_all(&link_target).unwrap();
    fs::write(link_target.join("package.json"), "{}").unwrap();
    pnpm_fs::symlink_dir(&link_target, &modules_dir.join("linked-dep")).unwrap();

    purge_modules_dir_entries(&modules_dir, &Config::new(), None).unwrap();

    let remaining = fs::read_dir(&modules_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    dbg!(&remaining);
    assert!(remaining.is_empty());
    assert!(link_target.join("package.json").exists(), "the purge must not follow the link");
}

/// A shim target spelled absolutely, as an earlier pnpm wrote every one.
const ABSOLUTE_TARGET: &str = "/elsewhere/project/node_modules/typescript/bin/tsc";

/// A shim target the tree carries with it.
#[cfg(unix)]
const RELATIVE_TARGET: &str = "../typescript/bin/tsc";

/// The `.bin` dirs a moved tree may hold a stale bin in, named from the
/// project root: the root importer's, [`PROJECT_DIR`]'s, and the hoist one.
const ROOT_BIN: &str = "node_modules/.bin";
const PROJECT_BIN: &str = "packages/a/node_modules/.bin";
const HOIST_BIN: &str = "node_modules/.pnpm/node_modules/.bin";

/// The `.bin` the injected-deps syncer writes next to a slot's package, in the
/// slot [`TWO_SLOTS`] records first.
const SLOT_PACKAGE_BIN: &str = "node_modules/.pnpm/a@1.0.0/node_modules/a/node_modules/.bin";

/// The `.bin` the hoisted linker writes inside a package it placed, and the
/// `hoistedLocations` entry [`short_circuits_over_a_bin`] records it at.
const NESTED_BIN: &str = "node_modules/nested/node_modules/.bin";
const HOISTED_LOCATION: &str = "node_modules/nested";

/// The workspace project [`short_circuits_over_a_bin`] records besides the
/// root.
const PROJECT_DIR: &str = "packages/a";

/// A lockfile with no slot to scan, and one whose second slot is clean, so a
/// scan one slot satisfies accepts a tree the other refuses.
const NO_SLOTS: &str = "lockfileVersion: '9.0'\nimporters: {}\n";
const TWO_SLOTS: &str =
    "lockfileVersion: '9.0'\nimporters: {}\nsnapshots:\n  a@1.0.0: {}\n  b@1.0.0: {}\n";

/// The `hoistedLocations` of a package the hoisted linker placed outside the
/// tree, and a record no install can have written.
const HOISTED_OUTSIDE: &str = "hoistedLocations:\n  a@1.0.0:\n    - ../outside\n";
const UNREADABLE_RECORD: &str = "hoistedLocations: [";

fn parse_lockfile(yaml: &str) -> Lockfile {
    serde_saphyr::from_str(yaml).expect("parse lockfile")
}

/// Whether the frozen short-circuit fires over an up-to-date `node_linker`
/// tree whose `bin_dir`, named from the project root, holds a shim naming
/// `target`.
fn short_circuits_over_a_bin(
    node_linker: NodeLinker,
    tree_moved: bool,
    bin_dir: &str,
    target: &str,
    state: Option<&WorkspaceState>,
) -> bool {
    let dir = tempdir().expect("create a temp dir");
    let project_root = dir.path().join("project");
    let mut config = Config::new();
    config.store_dir = dir.path().join("store").into();
    config.modules_dir = project_root.join("node_modules");
    config.virtual_store_dir = config.modules_dir.join(".pnpm");
    let config = config.leak();
    let bin_dir = project_root.join(bin_dir);
    fs::create_dir_all(&bin_dir).expect("create the bin dir");
    fs::write(bin_dir.join("tsc"), format!("#!/bin/sh\n# cmd-shim-target={target}\n"))
        .expect("write the shim");
    fs::create_dir_all(&config.modules_dir).expect("create the modules dir");
    fs::write(
        config.modules_dir.join(pnpm_modules_yaml::MODULES_FILENAME),
        format!("hoistedLocations:\n  nested@1.0.0:\n    - {HOISTED_LOCATION}\n"),
    )
    .expect("write the modules manifest");
    let lockfile = parse_lockfile(NO_SLOTS);
    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: true,
    };
    let modules = ModulesLayout {
        hoist_pattern: config.hoist_pattern.clone(),
        included,
        layout_version: Some(LayoutVersion),
        node_linker: Some(match node_linker {
            NodeLinker::Hoisted => pnpm_modules_yaml::NodeLinker::Hoisted,
            _ => pnpm_modules_yaml::NodeLinker::Isolated,
        }),
        pruned_at: httpdate::fmt_http_date(SystemTime::now()),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config
            .effective_virtual_store_dir()
            .to_string_lossy()
            .into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        ..Default::default()
    };
    let manifest =
        PackageManifest::from_value(project_root.join("package.json"), serde_json::json!({}));
    let projects = [(project_root.clone(), &manifest), (project_root.join(PROJECT_DIR), &manifest)];
    frozen_tree_up_to_date(&FrozenTreeUpToDate {
        tree: ModulesTreeContext { config, workspace_root: &project_root, node_linker, included },
        repeat: RepeatInstallPolicy {
            frozen: true,
            filtered: false,
            disable_optimistic_check: false,
            supported_architectures: None,
            rebuild: None,
            effective_node_version: None,
        },
        lockfile: Some(&lockfile),
        current_lockfile: Some(&lockfile),
        modules_manifest: Some(&modules),
        recorded: RecordedWorkspace { state, moved: tree_moved, projects: &projects },
    })
    .is_some()
}

/// Every `.bin` a moved tree may hold a stale bin in is checked, under the
/// linker that writes it.
#[test]
fn a_moved_tree_with_an_absolute_bin_is_refused() {
    let cases = [
        (NodeLinker::Isolated, ROOT_BIN),
        (NodeLinker::Isolated, PROJECT_BIN),
        (NodeLinker::Isolated, HOIST_BIN),
        (NodeLinker::Hoisted, NESTED_BIN),
    ];
    for (node_linker, bin_dir) in cases {
        assert!(
            !short_circuits_over_a_bin(
                node_linker,
                true,
                bin_dir,
                ABSOLUTE_TARGET,
                Some(&WorkspaceState::default()),
            ),
            "{node_linker:?} {bin_dir}",
        );
    }
}

/// A tree in place keeps its short-circuit whatever its bins name.
#[test]
fn frozen_short_circuit_unchanged_in_place() {
    assert!(short_circuits_over_a_bin(
        NodeLinker::Isolated,
        false,
        ROOT_BIN,
        ABSOLUTE_TARGET,
        Some(&WorkspaceState::default()),
    ));
}

#[test]
fn frozen_short_circuit_requires_recorded_workspace_state() {
    for node_linker in [NodeLinker::Isolated, NodeLinker::Hoisted] {
        assert!(
            !short_circuits_over_a_bin(node_linker, false, ROOT_BIN, ABSOLUTE_TARGET, None),
            "{node_linker:?}: without state the absolute shim's original root is unknown",
        );
    }
}

/// A moved tree whose bins name their targets relative to themselves is
/// reused, under the hoisted linker as under the isolated one.
#[cfg(unix)]
#[test]
fn a_moved_tree_with_relative_bins_is_reused() {
    for node_linker in [NodeLinker::Isolated, NodeLinker::Hoisted] {
        assert!(
            short_circuits_over_a_bin(
                node_linker,
                true,
                ROOT_BIN,
                RELATIVE_TARGET,
                Some(&WorkspaceState::default()),
            ),
            "{node_linker:?}",
        );
    }
}

/// Whether the move proof accepts a [`TWO_SLOTS`] tree under `node_linker`
/// whose `.modules.yaml` holds `modules_yaml`, with a shim naming
/// [`ABSOLUTE_TARGET`] in `absolute_bin`, named from the project root, when
/// there is one.
fn moved_tree_is_reusable_over(
    node_linker: NodeLinker,
    modules_yaml: &str,
    absolute_bin: Option<&str>,
) -> bool {
    let dir = tempdir().expect("create a temp dir");
    let project_root = dir.path().join("project");
    let mut config = Config::new();
    config.modules_dir = project_root.join("node_modules");
    config.virtual_store_dir = config.modules_dir.join(".pnpm");
    fs::create_dir_all(&config.modules_dir).expect("create the modules dir");
    fs::write(config.modules_dir.join(pnpm_modules_yaml::MODULES_FILENAME), modules_yaml)
        .expect("write the modules manifest");
    if let Some(absolute_bin) = absolute_bin {
        let bin_dir = project_root.join(absolute_bin);
        fs::create_dir_all(&bin_dir).expect("create the bin dir");
        fs::write(bin_dir.join("tsc"), format!("#!/bin/sh\n# cmd-shim-target={ABSOLUTE_TARGET}\n"))
            .expect("write the shim");
    }
    let manifest =
        PackageManifest::from_value(project_root.join("package.json"), serde_json::json!({}));
    moved_tree_is_reusable(
        &config,
        node_linker,
        &[(project_root, &manifest)],
        &parse_lockfile(TWO_SLOTS),
    )
}

/// Every `.bin` the proof must scan is scanned, each slot the lockfile records
/// included, and a `hoistedLocations` record it cannot trust refuses the move.
#[test]
fn a_moved_tree_is_refused_unless_every_bin_it_must_scan_is_relative() {
    assert_eq!(
        moved_tree_is_reusable_over(NodeLinker::Isolated, "", None),
        cfg!(unix),
        "a tree with no bin to refuse is reusable wherever a moved tree may be",
    );

    let cases = [
        ("the bin beside a slot's package", NodeLinker::Isolated, "", Some(SLOT_PACKAGE_BIN)),
        ("a package hoisted outside the tree", NodeLinker::Hoisted, HOISTED_OUTSIDE, None),
        ("an unreadable hoist record", NodeLinker::Hoisted, UNREADABLE_RECORD, None),
    ];
    for (label, node_linker, modules_yaml, absolute_bin) in cases {
        assert!(!moved_tree_is_reusable_over(node_linker, modules_yaml, absolute_bin), "{label}");
    }
}

/// The move signal is gated as the move proof is: where a moved tree is
/// never reused, a tree recorded elsewhere takes the path of one in place.
/// A workspace keeping one recorded project in place is not elsewhere.
#[test]
fn a_tree_recorded_elsewhere_is_moved_only_where_a_moved_tree_may_be_reused() {
    let project_root = Path::new("/here/project");
    let manifest =
        PackageManifest::from_value(project_root.join("package.json"), serde_json::json!({}));
    let projects = [(project_root.to_path_buf(), &manifest)];
    let state = WorkspaceState {
        projects: BTreeMap::from([("/elsewhere/project".to_string(), ProjectEntry::default())]),
        ..WorkspaceState::default()
    };
    let mut config = Config::new();
    config.modules_dir = project_root.join("node_modules");

    let moved =
        recorded_workspace(Some(&state), true, &config, NodeLinker::Isolated, &projects).moved;
    assert_eq!(moved, cfg!(unix));

    let in_place = WorkspaceState {
        projects: BTreeMap::from([(
            project_root.to_string_lossy().into_owned(),
            ProjectEntry::default(),
        )]),
        ..WorkspaceState::default()
    };
    let gained = [projects[0].clone(), (project_root.join("packages/b"), &manifest)];
    let moved =
        recorded_workspace(Some(&in_place), true, &config, NodeLinker::Isolated, &gained).moved;
    assert!(!moved, "a workspace that gained a project in place is recorded here");

    let lockfile = parse_lockfile(NO_SLOTS);
    config.enable_global_virtual_store = true;
    let moved =
        recorded_workspace(Some(&state), true, &config, NodeLinker::Isolated, &projects).moved;
    assert!(!moved, "a global virtual store never reuses a moved tree");
    assert!(
        !moved_tree_is_reusable(&config, NodeLinker::Isolated, &projects, &lockfile),
        "nor does its proof of a move",
    );

    config.enable_global_virtual_store = false;
    let moved = recorded_workspace(Some(&state), true, &config, NodeLinker::Pnp, &projects).moved;
    assert!(!moved, "pnp never reuses a moved tree");
    assert!(
        !moved_tree_is_reusable(&config, NodeLinker::Pnp, &projects, &lockfile),
        "nor does its proof of a move",
    );
}

#[test]
fn missing_or_corrupt_state_requires_relinking_only_for_an_existing_tree() {
    for state_contents in [None, Some("{")] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let mut config = Config::new();
        config.modules_dir = root.join("node_modules");
        fs::create_dir_all(&config.modules_dir).unwrap();
        if let Some(contents) = state_contents {
            fs::write(pnpm_workspace_state::get_file_path(root), contents).unwrap();
        }
        let manifest =
            PackageManifest::from_value(root.join("package.json"), serde_json::json!({}));
        let projects = [(root.to_path_buf(), &manifest)];
        let state = load_workspace_state(root).ok().flatten();
        assert!(state.is_none(), "the fixture must not provide a readable state");

        let existing =
            recorded_workspace(state.as_ref(), true, &config, NodeLinker::Isolated, &projects);
        assert_eq!(existing.moved, cfg!(unix), "the old bins may name another root");
        let fresh =
            recorded_workspace(state.as_ref(), false, &config, NodeLinker::Isolated, &projects);
        assert!(!fresh.moved, "a first filtered install must still write its initial state");
    }
}
