pub use _utils::*;

use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pipe_trait::Pipe;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::get_all_files,
    registry::RecordedRequest,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, process::Command};

#[test]
fn store_usable_by_pnpm_offline() {
    let CommandTempCwd {
        pacquet,
        pnpm,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Using pacquet to populate the store...");
    pacquet
        .with_arg("install")
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("delete node_modules");

    eprintln!("pnpm install --offline --ignore-scripts");
    pnpm.with_args(["install", "--offline", "--ignore-scripts"])
        .assert()
        .success();

    drop((root, mock_instance));
}

#[test]
fn same_file_structure() {
    let CommandTempCwd {
        pacquet,
        pnpm,
        root,
        workspace,
        npmrc_info,
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    let modules_dir = workspace.join("node_modules");
    // Cleanup also drops `pnpm-lock.yaml` because the fresh-lockfile
    // install path writes one, and leaving it would let pnpm's second
    // install pick a different code path (frozen-with-existing-lockfile)
    // than pacquet's first install (fresh), which the
    // `.pnpm-needs-build-marker` artifact in the GVS store difference
    // would surface as a spurious diff here.
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let cleanup = || {
        eprintln!("Cleaning up...");
        fs::remove_dir_all(&store_dir).expect("delete store dir");
        fs::remove_dir_all(&modules_dir).expect("delete node_modules");
        let _ = fs::remove_file(&lockfile_path);
    };

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(manifest_path, package_json_content.to_string()).expect("write to package.json");

    // Filter out pnpm-only artifacts whose presence is orthogonal to whether
    // the two tools agree on the CAFS layout:
    //   * `v11/projects/<hash>` — pnpm-11-only per-project metadata tracking
    //     which packages in the store are linked from which project. Sharing
    //     the store doesn't require it.
    //   * `v11/index.db-wal` / `v11/index.db-shm` — SQLite WAL sidecars that
    //     only exist while a connection is open; their presence at comparison
    //     time depends on whether the checkpoint ran before we measured.
    let normalize = |files: Vec<String>| -> Vec<String> {
        files
            .into_iter()
            // Per-project metadata that pnpm 11 populates and pacquet doesn't.
            // Doesn't affect the shared-cafs story.
            .filter(|path| !path.starts_with("v11/projects/"))
            // Hoisted-symlinks layout introduced in pnpm 11 — pnpm stores
            // one `node_modules` tree per `<name>/<version>/<hash>/` under
            // `v11/links/` and links the project's `node_modules/X` into there.
            // Pacquet still uses the older per-project `.pnpm/` virtual store,
            // so these paths exist only on the pnpm side.
            .filter(|path| !path.starts_with("v11/links/"))
            // SQLite WAL sidecars exist only while a connection holds the
            // journal open. Their presence at compare-time depends on timing.
            .filter(|path| path != "v11/index.db-wal" && path != "v11/index.db-shm")
            .collect()
    };

    eprintln!("Installing with pacquet...");
    pacquet
        .with_arg("install")
        .assert()
        .success();
    let pacquet_store_files = normalize(get_all_files(&store_dir));

    cleanup();

    eprintln!("Installing with pnpm...");
    pnpm.with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    let pnpm_store_files = normalize(get_all_files(&store_dir));

    cleanup();

    eprintln!("Produce the same store dir structure");
    assert_eq!(&pacquet_store_files, &pnpm_store_files);

    drop((root, mock_instance));
}

// Both pnpm and pacquet write `index.db` values as msgpackr
// records (pnpm via `Packr({useRecords: true})`, pacquet via
// `encode_package_files_index`). `StoreIndex::get` decodes both through
// the shared transcoder, so this test just asserts the two tools'
// decoded `PackageFilesIndex` shapes match for the same install — not
// byte-identical rows, because `HashMap` iteration order can differ
// from msgpackr's, but the post-decode structs compare equal.
#[test]
fn same_index_file_contents() {
    let CommandTempCwd {
        pacquet,
        pnpm,
        root,
        workspace,
        npmrc_info,
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    let modules_dir = workspace.join("node_modules");
    // Cleanup also drops `pnpm-lock.yaml` so pnpm doesn't pick a
    // different install code path than pacquet on the second leg —
    // see the matching note in `same_file_structure`.
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let cleanup = || {
        eprintln!("Cleaning up...");
        fs::remove_dir_all(&store_dir).expect("delete store dir");
        fs::remove_dir_all(&modules_dir).expect("delete node_modules");
        let _ = fs::remove_file(&lockfile_path);
    };

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Installing with pacquet...");
    pacquet
        .with_arg("install")
        .assert()
        .success();
    let pacquet_index_file_contents = store_dir
        .pipe_as_ref(index_file_contents)
        .pipe(serde_json::to_value)
        .expect("serialize pacquet index file contents");

    cleanup();

    eprintln!("Installing with pnpm...");
    pnpm.with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    let pnpm_index_file_contents = store_dir
        .pipe_as_ref(index_file_contents)
        .pipe(serde_json::to_value)
        .expect("serialize pnpm index file contents");

    cleanup();

    eprintln!("Produce the same store dir structure");
    assert_eq!(&pacquet_index_file_contents, &pnpm_index_file_contents);

    drop((root, mock_instance));
}

// Regression: pacquet-written `index.db` rows must remain readable
// by pnpm's msgpackr-based reader. Pacquet writes
// msgpackr-records via `encode_package_files_index`; this test guards
// against regressing to the `rmp_serde::to_vec_named` plain-map
// encoding.
//
// Why that regression would be silent without this test: pnpm's
// `Packr({useRecords: true, moreTypes: true}).unpack(...)` decodes
// every plain msgpack map (at any nesting level) as a JS `Map` —
// records are the escape hatch that says "this one's a plain object".
// A plain-map-encoded row would come back as a top-level `Map`,
// `pkgIndex.files` (property access) would be `undefined`, and pnpm's
// `for (const [f, fstat] of pkgIndex.files)` would throw
// `files is not iterable`, surfacing as `ERR_PNPM_READ_FROM_STORE`.
//
// The flow below reproduces the benchmark's path: pacquet populates
// the store, `node_modules` is wiped, then pnpm installs against the
// same store. Leaving the store intact — unlike `same_file_structure`
// and `same_index_file_contents`, which clean it between the pacquet
// and pnpm halves — is what makes pnpm actually *read* pacquet's
// rows.
#[test]
fn pnpm_reads_pacquet_written_rows() {
    let CommandTempCwd {
        pacquet,
        pnpm,
        root,
        workspace,
        npmrc_info,
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("pacquet install (populates store with msgpackr records)...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    eprintln!("Removing node_modules; store is kept so pnpm has to read pacquet's rows...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("delete node_modules");

    eprintln!("pnpm install --ignore-scripts (reads pacquet's index.db rows)...");
    pnpm.with_args(["install", "--ignore-scripts"])
        .assert()
        .success();

    drop((root, mock_instance));
}

/// Filter a full store-dir listing down to the GVS slot subtree.
///
/// pnpm writes GVS slots under `v11/links/<scope>/<name>/<version>/<hash>/...`
/// because it appends `STORE_VERSION` (`"v11"`) to the user-configured
/// `storeDir`.
/// Pacquet's [`StoreDir::links`](../../../../store-dir/src/store_dir.rs)
/// puts them at `links/<scope>/<name>/<version>/<hash>/...` — one level
/// shallower. Both prefixes pass through unmodified, so when the two
/// path sets are diffed in `assert_eq!` the prefix divergence shows up
/// alongside any inner-shape disagreement instead of being silently
/// normalized away.
fn gvs_paths_only(files: Vec<String>) -> Vec<String> {
    files
        .into_iter()
        .filter(|path| path.starts_with("links/") || path.starts_with("v11/links/"))
        .collect()
}

/// Run pnpm-then-pacquet against a shared workspace and compare the
/// GVS slot trees they each materialize. Pnpm runs first so the
/// lockfile exists before pacquet starts — pacquet's GVS write path
/// is gated on `frozen_lockfile && enable_global_virtual_store` (see
/// `package-manager/src/install.rs:299` and the
/// [`VirtualStoreLayout::legacy`](../../../../package-manager/src/virtual_store_layout.rs)
/// docstring), so a fresh install with no lockfile would silently fall
/// through to the project-local layout and the test would pass for the
/// wrong reason.
///
/// Caller passes `pnpm_extra_args` so individual tests can add things
/// like `--ignore-scripts` without hard-coding it here. The store and
/// `node_modules` are wiped between the two installs so pacquet writes
/// the slot tree from scratch rather than reading pnpm's leftovers.
fn install_then_compare_gvs(
    pnpm: std::process::Command,
    pacquet: std::process::Command,
    store_dir: &std::path::Path,
    modules_dir: &std::path::Path,
    pnpm_extra_args: &[&str],
) {
    let mut pnpm_args = vec!["install"];
    pnpm_args.extend_from_slice(pnpm_extra_args);
    eprintln!("Installing with pnpm (writes lockfile + pnpm-side GVS slots)...");
    pnpm.with_args(pnpm_args)
        .assert()
        .success();
    let pnpm_gvs_paths = gvs_paths_only(get_all_files(store_dir));
    assert!(
        !pnpm_gvs_paths.is_empty(),
        "pnpm must have written GVS slots; got nothing matching v11/links/ or links/",
    );

    eprintln!("Wiping store + node_modules (keeping lockfile so pacquet runs in frozen mode)...");
    fs::remove_dir_all(store_dir).expect("delete store dir");
    fs::remove_dir_all(modules_dir).expect("delete node_modules");

    eprintln!("Installing with pacquet --frozen-lockfile (writes pacquet-side GVS slots)...");
    pacquet
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    let pacquet_gvs_paths = gvs_paths_only(get_all_files(store_dir));

    eprintln!("Comparing GVS layouts (pnpm on the right, pacquet on the left)...");
    assert_eq!(&pacquet_gvs_paths, &pnpm_gvs_paths);
}

/// Pure-JS GVS parity: a package with one transitive dep, no install
/// scripts. With `allowBuilds` left at the GVS default of `{}` —
/// it defaults to `{}` whenever `enableGlobalVirtualStore` is on — every
/// snapshot hashes with `engine = null`, so the GVS slot tree is
/// engine-agnostic and the comparison is independent of the host
/// Node.js / OS / arch the test runs on.
#[test]
fn same_global_virtual_store_layout_pure_js() {
    let CommandTempCwd {
        pacquet,
        pnpm,
        root,
        workspace,
        npmrc_info,
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    enable_gvs_in_workspace_yaml(&workspace, "");

    eprintln!("Creating package.json...");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    install_then_compare_gvs(
        pnpm,
        pacquet,
        &store_dir,
        &workspace.join("node_modules"),
        &["--ignore-scripts"],
    );

    drop((root, mock_instance));
}

/// Engine-included GVS parity: `pre-and-postinstall-scripts-example`
/// has install scripts and is explicitly approved via `allowBuilds`,
/// so it lands in the built-dependency set and its GVS hash
/// includes the `ENGINE_NAME` string.
/// Pacquet's
/// [`calc_graph_node_hash`](../../../../graph-hasher/src/global_virtual_store_path.rs)
/// must produce the same engine-included digest, or pnpm and pacquet
/// would split the same approved-build package across two slot
/// directories.
///
/// Scripts run on both sides (neither install uses `--ignore-scripts`) —
/// if pnpm skipped scripts while pacquet ran them the slot trees would
/// diverge on the script-generated `generated-by-*.js` files even
/// though the hash itself agreed.
#[test]
fn same_global_virtual_store_layout_with_approved_postinstall() {
    let CommandTempCwd {
        pacquet,
        pnpm,
        root,
        workspace,
        npmrc_info,
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    enable_gvs_in_workspace_yaml(
        &workspace,
        "allowBuilds:\n  '@pnpm.e2e/pre-and-postinstall-scripts-example': true\n",
    );

    eprintln!("Creating package.json...");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    install_then_compare_gvs(
        pnpm,
        pacquet,
        &store_dir,
        &workspace.join("node_modules"),
        &[], // scripts must run on both sides; see fn doc above
    );

    drop((root, mock_instance));
}

/// Diamond GVS parity: the root depends on both `pkg-with-1-dep` and
/// `parent-of-pkg-with-1-dep`, and `parent-of-pkg-with-1-dep` itself
/// depends on `pkg-with-1-dep`. So `pkg-with-1-dep` is reachable
/// through two paths from the root, and `calc_dep_graph_hash` must
/// hit its memoization cache on the second visit — if the cache key
/// or the hash payload disagreed between pnpm and pacquet, the
/// `pkg-with-1-dep` slot would land at one path on pnpm and another
/// on pacquet. Mirrors the cache-correctness guarantee that the unit
/// test [`diamond_graph_resolves_consistently`](../../../../graph-hasher/src/dep_state.rs)
/// already covers in isolation, here exercised through the full
/// install pipeline.
#[test]
fn same_global_virtual_store_layout_diamond() {
    let CommandTempCwd {
        pacquet,
        pnpm,
        root,
        workspace,
        npmrc_info,
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    enable_gvs_in_workspace_yaml(&workspace, "");

    eprintln!("Creating package.json...");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "@pnpm.e2e/parent-of-pkg-with-1-dep": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    install_then_compare_gvs(
        pnpm,
        pacquet,
        &store_dir,
        &workspace.join("node_modules"),
        &["--ignore-scripts"],
    );

    drop((root, mock_instance));
}

#[derive(Debug, Clone, Copy)]
struct DifferentialRow {
    shared_lockfile: bool,
    hoisted: bool,
    inject_workspace_packages: bool,
    auto_install_peers: bool,
    dedupe_peer_dependents: bool,
    auto_dedupe: bool,
}

impl DifferentialRow {
    fn workspace_yaml(self) -> String {
        let mut yaml = format!(
            "\npackages:\n  - 'packages/*'\nsharedWorkspaceLockfile: {}\nnodeLinker: {}\n\
             injectWorkspacePackages: {}\nautoInstallPeers: {}\ndedupePeerDependents: {}\n",
            self.shared_lockfile,
            if self.hoisted { "hoisted" } else { "isolated" },
            self.inject_workspace_packages,
            self.auto_install_peers,
            self.dedupe_peer_dependents,
        );
        if self.auto_dedupe {
            yaml.push_str("autoDedupe: true\n");
        }
        yaml
    }
}

#[derive(Debug, PartialEq, Eq)]
struct NormalizedWorkspaceState {
    projects: BTreeMap<String, pnpm_workspace_state::ProjectEntry>,
    filtered_install: bool,
    config_dependencies: Option<BTreeMap<String, pnpm_workspace_state::ConfigDependency>>,
    validation: Option<pnpm_workspace_state::Validation>,
}

#[derive(Debug, PartialEq, Eq)]
struct DifferentialSnapshot {
    tree: BTreeMap<String, String>,
    lockfiles: BTreeMap<String, String>,
    modules: BTreeMap<String, pnpm_modules_yaml::Modules>,
    state: Option<NormalizedWorkspaceState>,
}

const DIFFERENTIAL_PROJECTS: [&str; 3] = ["a", "b", "c"];

fn write_differential_workspace(workspace: &Path, row: DifferentialRow) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let base_yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&yaml_path, format!("{base_yaml}{}", row.workspace_yaml()))
        .expect("write pnpm-workspace.yaml");

    write_project_manifest(
        workspace,
        "root",
        ManifestDeps {
            prod: &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0")],
            ..ManifestDeps::default()
        },
    );
    write_project_manifest(
        &workspace.join("packages/a"),
        "a",
        ManifestDeps {
            prod: &[("@pnpm.e2e/foo", "100.0.0")],
            peer: &[("@pnpm.e2e/peer-a", "^1.0.0")],
            ..ManifestDeps::default()
        },
    );
    write_project_manifest(
        &workspace.join("packages/b"),
        "b",
        ManifestDeps {
            prod: &[
                ("a", "workspace:*"),
                ("@pnpm.e2e/peer-a", "1.0.0"),
                ("@pnpm.e2e/abc-parent-with-ab", "1.0.0"),
            ],
            dev: &[("@pnpm.e2e/bar", "100.0.0")],
            ..ManifestDeps::default()
        },
    );
    write_project_manifest(
        &workspace.join("packages/c"),
        "c",
        ManifestDeps {
            prod: &[("@pnpm.e2e/abc-parent-with-ab", "1.0.0"), ("@pnpm.e2e/peer-c", "1.0.1")],
            ..ManifestDeps::default()
        },
    );
}

fn read_normalized_workspace_state(workspace: &Path) -> Option<NormalizedWorkspaceState> {
    let text = fs::read_to_string(pnpm_workspace_state::get_file_path(workspace)).ok()?;
    let state: pnpm_workspace_state::WorkspaceState =
        serde_json::from_str(&text).expect("parse .pnpm-workspace-state-v1.json");
    let projects = state.projects
        .into_iter()
        .map(|(dir, entry)| {
            let rel = Path::new(&dir)
                .strip_prefix(workspace)
                .unwrap_or_else(|_| Path::new(&dir))
                .to_string_lossy()
                .replace('\\', "/");
            (rel, entry)
        })
        .collect();
    let validation = state.validation.map(|mut val| {
        val.recorded_at_ns = 0;
        val.files = val.files
            .into_iter()
            .map(|(file_path, mut sig)| {
                assert!(
                    sig.ctime_ns.is_some(),
                    "file signature for {file_path} must record ctimeNs",
                );
                sig.mtime_ns = 0;
                sig.ctime_ns = Some(0);
                (file_path.replace('\\', "/"), sig)
            })
            .collect();
        val
    });
    Some(NormalizedWorkspaceState {
        projects,
        filtered_install: state.filtered_install,
        config_dependencies: state.config_dependencies,
        validation,
    })
}

fn differential_snapshot(workspace: &Path, shared_lockfile: bool) -> DifferentialSnapshot {
    let mut resolver = TreeView::new(workspace);
    let mut tree = BTreeMap::from([(".".to_string(), resolver.project(workspace, true))]);
    for project in DIFFERENTIAL_PROJECTS {
        let dir = workspace.join("packages").join(project);
        tree.insert(project.to_string(), resolver.project(&dir, true));
    }
    DifferentialSnapshot {
        tree,
        lockfiles: workspace_lockfiles(workspace),
        modules: workspace_modules_manifests(workspace),
        state: shared_lockfile.then(|| read_normalized_workspace_state(workspace)).flatten(),
    }
}

fn wipe_workspace_install_outputs(
    workspace: &Path,
    store_dir: &Path,
    cache_dir: &Path,
    keep_lockfiles: bool,
) {
    let _ = fs::remove_dir_all(store_dir);
    let _ = fs::remove_dir_all(cache_dir);
    let _ = fs::remove_dir_all(workspace.join("node_modules"));
    for project in DIFFERENTIAL_PROJECTS {
        let _ = fs::remove_dir_all(
            workspace
                .join("packages")
                .join(project)
                .join("node_modules"),
        );
    }
    if !keep_lockfiles {
        for path in workspace_lockfiles(workspace).keys() {
            let _ = fs::remove_file(workspace.join(path));
        }
    }
}

fn pnpm_command_in(workspace: &Path) -> Command {
    let bundle = read_v11_bundle_path().expect("PNPM_V11_BUNDLE must be set");
    let mut cmd = v11_in(&bundle, workspace);
    cmd.arg("--config.confirm-modules-purge=false");
    cmd
}

fn check_differential_row(row: DifferentialRow) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry {
        store_dir, cache_dir, mock_instance, ..
    } = npmrc_info;

    write_differential_workspace(&workspace, row);

    let args = ["install", "--no-frozen-lockfile", "--ignore-scripts"];
    let run_gate_args = ["--config.verify-deps-before-run=error", "exec", "node", "-e", ""];

    pacquet_in(&workspace)
        .with_args(args)
        .assert()
        .success();
    let pacquet_snapshot = differential_snapshot(&workspace, row.shared_lockfile);
    if row.shared_lockfile {
        pnpm_command_in(&workspace)
            .with_args(run_gate_args)
            .assert()
            .success();
    }

    if row.auto_dedupe {
        // `autoDedupe` is a v12-only setting (`default: false`). After a v12
        // install, v11 must accept the workspace on its repeat-install fast
        // path and preserve `settings.autoDedupe` so a subsequent v12 run gate
        // still passes, and v11 `--frozen-lockfile` over v12's wanted lockfile
        // must materialize the same snapshot.
        pnpm_command_in(&workspace)
            .with_args(args)
            .assert()
            .success();
        pacquet_in(&workspace)
            .with_args(run_gate_args)
            .assert()
            .success();
        assert_eq!(
            differential_snapshot(&workspace, row.shared_lockfile),
            pacquet_snapshot,
            "v11 repeat install over v12 autoDedupe workspace diverged for {row:?}",
        );

        wipe_workspace_install_outputs(&workspace, &store_dir, &cache_dir, true);
        pnpm_command_in(&workspace)
            .with_args(["install", "--frozen-lockfile", "--ignore-scripts"])
            .assert()
            .success();
        assert_eq!(
            differential_snapshot(&workspace, row.shared_lockfile),
            pacquet_snapshot,
            "v11 frozen install over v12 autoDedupe lockfile diverged for {row:?}",
        );
        drop((root, mock_instance));
        return;
    }

    wipe_workspace_install_outputs(&workspace, &store_dir, &cache_dir, false);

    pnpm_command_in(&workspace)
        .with_args(args)
        .assert()
        .success();
    let pnpm_snapshot = differential_snapshot(&workspace, row.shared_lockfile);
    if row.shared_lockfile {
        pacquet_in(&workspace)
            .with_args(run_gate_args)
            .assert()
            .success();
    }

    assert_eq!(pacquet_snapshot, pnpm_snapshot, "v12 and v11 snapshots diverged for {row:?}");

    // Cross-CLI frozen handoff: v12 materializes from v11's wanted lockfiles.
    wipe_workspace_install_outputs(&workspace, &store_dir, &cache_dir, true);
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--ignore-scripts"])
        .assert()
        .success();
    assert_eq!(
        differential_snapshot(&workspace, row.shared_lockfile),
        pnpm_snapshot,
        "v12 frozen install over v11 lockfile diverged for {row:?}",
    );

    // Cross-CLI frozen handoff: v11 materializes from v12's wanted lockfiles.
    wipe_workspace_install_outputs(&workspace, &store_dir, &cache_dir, false);
    pacquet_in(&workspace)
        .with_args(args)
        .assert()
        .success();
    wipe_workspace_install_outputs(&workspace, &store_dir, &cache_dir, true);
    pnpm_command_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--ignore-scripts"])
        .assert()
        .success();
    assert_eq!(
        differential_snapshot(&workspace, row.shared_lockfile),
        pacquet_snapshot,
        "v11 frozen install over v12 lockfile diverged for {row:?}",
    );

    drop((root, mock_instance));
}

#[test]
#[ignore = "requires PNPM_V11_BUNDLE; run with --run-ignored all in the convergence job"]
fn same_lockfiles_modules_and_tree_isolated_shared() {
    for (auto_install_peers, dedupe_peer_dependents, auto_dedupe) in [
        (true, true, false),
        (true, true, true),
        (true, false, false),
        (false, true, false),
        (false, false, false),
    ] {
        check_differential_row(DifferentialRow {
            shared_lockfile: true,
            hoisted: false,
            inject_workspace_packages: false,
            auto_install_peers,
            dedupe_peer_dependents,
            auto_dedupe,
        });
    }
}

#[test]
#[ignore = "requires PNPM_V11_BUNDLE; run with --run-ignored all in the convergence job"]
fn same_lockfiles_modules_and_tree_isolated_injected_and_dedicated() {
    for (shared_lockfile, inject_workspace_packages) in
        [(true, true), (false, false), (false, true)]
    {
        check_differential_row(DifferentialRow {
            shared_lockfile,
            hoisted: false,
            inject_workspace_packages,
            auto_install_peers: true,
            dedupe_peer_dependents: true,
            auto_dedupe: false,
        });
    }
}

fn invoke_cli_in(workspace: &Path, use_pnpm: bool, args: &[&str]) {
    let cmd = if use_pnpm { pnpm_command_in(workspace) } else { pacquet_in(workspace) };
    cmd.with_args(args).assert().success();
}

fn run_filtered_install_after_edit(
    workspace: &Path,
    store_dir: &Path,
    cache_dir: &Path,
    yaml_path: &Path,
    base_yaml: &str,
    row: DifferentialRow,
    use_pnpm: bool,
) -> DifferentialSnapshot {
    wipe_workspace_install_outputs(workspace, store_dir, cache_dir, false);
    fs::write(yaml_path, base_yaml).expect("reset pnpm-workspace.yaml");
    write_differential_workspace(workspace, row);
    invoke_cli_in(workspace, use_pnpm, &["install", "--no-frozen-lockfile", "--ignore-scripts"]);
    set_dependency(&workspace.join("packages/b"), "dependencies", "@pnpm.e2e/foo", "100.0.0");
    invoke_cli_in(
        workspace,
        use_pnpm,
        &["install", "--no-frozen-lockfile", "--ignore-scripts", "--filter", "b"],
    );
    differential_snapshot(workspace, row.shared_lockfile)
}

#[test]
#[ignore = "requires PNPM_V11_BUNDLE; run with --run-ignored all in the convergence job"]
fn same_lockfiles_modules_and_tree_hoisted_and_filtered() {
    for (shared_lockfile, inject_workspace_packages) in
        [(true, false), (true, true), (false, false), (false, true)]
    {
        let row = DifferentialRow {
            shared_lockfile,
            hoisted: true,
            inject_workspace_packages,
            auto_install_peers: true,
            dedupe_peer_dependents: true,
            auto_dedupe: false,
        };
        check_differential_row(row);
    }

    for (hoisted, shared_lockfile) in [(false, true), (false, false), (true, true), (true, false)] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry {
            store_dir, cache_dir, mock_instance, ..
        } = npmrc_info;
        let row = DifferentialRow {
            shared_lockfile,
            hoisted,
            inject_workspace_packages: false,
            auto_install_peers: true,
            dedupe_peer_dependents: true,
            auto_dedupe: false,
        };

        let yaml_path = workspace.join("pnpm-workspace.yaml");
        let base_yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");

        let pacquet_snapshot = run_filtered_install_after_edit(
            &workspace, &store_dir, &cache_dir, &yaml_path, &base_yaml, row, false,
        );
        let pnpm_snapshot = run_filtered_install_after_edit(
            &workspace, &store_dir, &cache_dir, &yaml_path, &base_yaml, row, true,
        );
        assert_eq!(
            pacquet_snapshot, pnpm_snapshot,
            "v12 and v11 filtered install after edit diverged for {row:?}",
        );

        drop((root, mock_instance));
    }
}

fn format_recorded_request(req: RecordedRequest) -> String {
    let RecordedRequest {
        method,
        path,
        accept,
        if_none_match,
        if_modified_since,
        status,
        ..
    } = req;
    let path = path.strip_prefix("/npm").unwrap_or(&path);
    if path.ends_with(".tgz") {
        return format!("{method} {path} {status}");
    }
    let is_abbreviated = accept
        .as_deref()
        .is_some_and(|header| header.starts_with("application/vnd.npm.install-v1+json"));
    let doc = if is_abbreviated { "abbreviated" } else { "full" };
    let etag = if_none_match
        .map(|val| format!(" etag={val}"))
        .unwrap_or_default();
    let since = if if_modified_since.is_some() { " since" } else { "" };
    format!("{method} {path} {doc}{etag}{since} {status}")
}

fn run_four_install_modes_with_request_logs(use_pnpm: bool) -> Vec<Vec<String>> {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_recording_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    let invoke = |args: &[&str]| {
        invoke_cli_in(&workspace, use_pnpm, args);
        let mut requests: Vec<String> = mock_instance
            .take_requests()
            .into_iter()
            .map(format_recorded_request)
            .collect();
        requests.sort();
        requests
    };

    // 1. Cold install: fetches packuments and tarballs.
    let cold = invoke(&["install", "--no-frozen-lockfile", "--ignore-scripts"]);
    // 2. Repeat install over populated node_modules + lockfile: 0 requests.
    let repeat = invoke(&["install", "--no-frozen-lockfile", "--ignore-scripts"]);
    // 3. Frozen install with warm store after wiping node_modules: 0 requests.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    let frozen_warm = invoke(&["install", "--frozen-lockfile", "--ignore-scripts"]);
    // 4. Fresh resolution with warm store and warm metadata cache: revalidates packuments (304), 0 tarballs.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    let warm_reresolve = invoke(&["install", "--no-frozen-lockfile", "--ignore-scripts"]);

    drop((root, mock_instance));
    vec![cold, repeat, frozen_warm, warm_reresolve]
}

#[test]
#[ignore = "requires PNPM_V11_BUNDLE; run with --run-ignored all in the convergence job"]
fn same_registry_request_logs_across_install_modes() {
    let pacquet_logs = run_four_install_modes_with_request_logs(false);
    let pnpm_logs = run_four_install_modes_with_request_logs(true);
    assert_eq!(
        pacquet_logs[1],
        Vec::<String>::new(),
        "repeat install must make no registry requests",
    );
    assert_eq!(
        pacquet_logs[2],
        Vec::<String>::new(),
        "frozen install with warm store must make no registry requests",
    );
    let warm_reresolve_tarballs: Vec<&String> = pacquet_logs[3]
        .iter()
        .filter(|line| line.contains(".tgz"))
        .collect();
    assert_eq!(
        warm_reresolve_tarballs,
        Vec::<&String>::new(),
        "warm re-resolution must not fetch tarballs",
    );
    assert_eq!(pacquet_logs, pnpm_logs, "v12 and v11 registry request logs diverged");
}
