#![cfg(unix)] // running this on windows result in 'program not found'
pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::get_all_files,
};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::fs;

#[test]
#[ignore = "requires metadata cache feature which pacquet doesn't yet have"]
fn store_usable_by_pnpm_offline() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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
    pacquet.with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("delete node_modules");

    eprintln!("pnpm install --offline --ignore-scripts");
    pnpm.with_args(["install", "--offline", "--ignore-scripts"]).assert().success();

    drop((root, mock_instance));
}

#[test]
fn same_file_structure() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info } =
        CommandTempCwd::init().add_mocked_registry();
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
    //     which packages in the store are linked from which project. Pacquet
    //     doesn't yet populate this, and sharing the store doesn't require it.
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
    pacquet.with_arg("install").assert().success();
    let pacquet_store_files = normalize(get_all_files(&store_dir));

    cleanup();

    eprintln!("Installing with pnpm...");
    pnpm.with_args(["install", "--ignore-scripts"]).assert().success();
    let pnpm_store_files = normalize(get_all_files(&store_dir));

    cleanup();

    eprintln!("Produce the same store dir structure");
    assert_eq!(&pacquet_store_files, &pnpm_store_files);

    drop((root, mock_instance));
}

// Both pnpm and pacquet now write `index.db` values as msgpackr
// records (pnpm via `Packr({useRecords: true})`, pacquet via
// `encode_package_files_index`). `StoreIndex::get` decodes both through
// the shared transcoder, so this test just asserts the two tools'
// decoded `PackageFilesIndex` shapes match for the same install — not
// byte-identical rows, because `HashMap` iteration order can differ
// from msgpackr's, but the post-decode structs compare equal.
#[test]
fn same_index_file_contents() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info } =
        CommandTempCwd::init().add_mocked_registry();
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
    pacquet.with_arg("install").assert().success();
    let pacquet_index_file_contents = store_dir
        .pipe_as_ref(index_file_contents)
        .pipe(serde_json::to_value)
        .expect("serialize pacquet index file contents");

    cleanup();

    eprintln!("Installing with pnpm...");
    pnpm.with_args(["install", "--ignore-scripts"]).assert().success();
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
// by pnpm's msgpackr-based reader. Pacquet now writes
// msgpackr-records via `encode_package_files_index`; this test guards
// against regressing to the older `rmp_serde::to_vec_named` plain-map
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
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info } =
        CommandTempCwd::init().add_mocked_registry();
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
    pacquet.with_arg("install").assert().success();

    eprintln!("Removing node_modules; store is kept so pnpm has to read pacquet's rows...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("delete node_modules");

    eprintln!("pnpm install --ignore-scripts (reads pacquet's index.db rows)...");
    pnpm.with_args(["install", "--ignore-scripts"]).assert().success();

    drop((root, mock_instance));
}

/// Filter a full store-dir listing down to the GVS slot subtree.
///
/// pnpm writes GVS slots under `v11/links/<scope>/<name>/<version>/<hash>/...`
/// because [`getStorePath`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L39-L42)
/// appends `STORE_VERSION` (`"v11"`) to the user-configured `storeDir`.
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
    pnpm.with_args(pnpm_args).assert().success();
    let pnpm_gvs_paths = gvs_paths_only(get_all_files(store_dir));
    assert!(
        !pnpm_gvs_paths.is_empty(),
        "pnpm must have written GVS slots; got nothing matching v11/links/ or links/",
    );

    eprintln!("Wiping store + node_modules (keeping lockfile so pacquet runs in frozen mode)...");
    fs::remove_dir_all(store_dir).expect("delete store dir");
    fs::remove_dir_all(modules_dir).expect("delete node_modules");

    eprintln!("Installing with pacquet --frozen-lockfile (writes pacquet-side GVS slots)...");
    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();
    let pacquet_gvs_paths = gvs_paths_only(get_all_files(store_dir));

    eprintln!("Comparing GVS layouts (pnpm on the right, pacquet on the left)...");
    assert_eq!(&pacquet_gvs_paths, &pnpm_gvs_paths);
}

/// Pure-JS GVS parity: a package with one transitive dep, no install
/// scripts. With `allowBuilds` left at the GVS default of `{}` —
/// upstream's
/// [`extendInstallOptions.ts:354`](https://github.com/pnpm/pnpm/blob/29a42efc3b/installing/deps-installer/src/install/extendInstallOptions.ts#L354)
/// applies `??= {}` whenever `enableGlobalVirtualStore` is on — every
/// snapshot hashes with `engine = null`, so the GVS slot tree is
/// engine-agnostic and the comparison is independent of the host
/// Node.js / OS / arch the test runs on.
#[test]
fn same_global_virtual_store_layout_pure_js() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info } =
        CommandTempCwd::init().add_mocked_registry();
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
/// so it lands in upstream's `builtDepPaths` set and its GVS hash
/// includes the `ENGINE_NAME` string (see
/// [`calcGraphNodeHash`](https://github.com/pnpm/pnpm/blob/29a42efc3b/deps/graph-hasher/src/index.ts#L140-L146)).
/// Pacquet's
/// [`calc_graph_node_hash`](../../../../graph-hasher/src/global_virtual_store_path.rs)
/// must produce the same engine-included digest, or pnpm and pacquet
/// would split the same approved-build package across two slot
/// directories.
///
/// Scripts run on both sides (neither install uses `--ignore-scripts`)
/// because pacquet doesn't expose `--ignore-scripts` yet
/// (pacquet/crates/cli/README.md lists it as a TODO) — if pnpm
/// skipped scripts while pacquet ran them the slot trees would
/// diverge on the script-generated `generated-by-*.js` files even
/// though the hash itself agreed.
///
/// **Ignored until a pnpm release ships the engine-name fix from
/// commit 8f05529c11.** This test requires pnpm and pacquet to agree
/// on the `<platform>;<arch>;node<major>` triple used in the
/// engine-included hash branch. Pre-fix pnpm anchored the value to
/// `process.version` — the Node embedded in the `@pnpm/exe` SEA
/// bundle on Linux/macOS CI runners, currently Node 26 — while
/// pacquet (and any non-SEA caller) detects the `node` on `PATH`,
/// which on GHA's standard runners is Node 24. The hash digests
/// therefore land at different majors and the slot paths diverge.
/// The pnpm-side fix in this PR resolves `engineName()` via
/// `getSystemNodeVersion()` which prefers the shell `node`, so once
/// a published pnpm version with that fix reaches
/// [`pnpm/setup`](https://github.com/pnpm/setup) the test will pass
/// without modification — re-enable it then.
#[test]
#[ignore = "depends on a published pnpm version that includes commit 8f05529c11; see test doc comment"]
fn same_global_virtual_store_layout_with_approved_postinstall() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info } =
        CommandTempCwd::init().add_mocked_registry();
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
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info } =
        CommandTempCwd::init().add_mocked_registry();
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
