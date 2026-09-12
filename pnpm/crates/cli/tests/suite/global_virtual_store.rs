//! Ports of the upstream global-virtual-store suites — the primary
//! `installing/deps-installer/test/install/globalVirtualStore.ts` and the
//! CLI-level `pnpm/test/install/globalVirtualStore.ts`.
//!
//! Upstream drives these through the programmatic `install()` API and can
//! monkey-patch `storeController.fetchPackage` to count fetches. Pacquet's
//! equivalent surface is the CLI, so the ports assert the same on-disk
//! contract — slot layout, hash-directory identity across `allowBuilds`
//! changes, build artifacts, and `.modules.yaml` state — instead of the
//! call counts. Where that loses a signal it is called out on the test.
//!
#![cfg(unix)] // the GVS slot assertions read symlinks

pub use _utils::*;

use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_store_dir::{STORE_VERSION, StoreDir, StoreIndex};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// `<store_dir>/v11/links` — the root every GVS slot hangs off.
fn gvs_root(store_dir: &Path) -> PathBuf {
    store_dir.join(STORE_VERSION).join("links")
}

/// The `<gvs>/<scope>/<name>/<version>` directory whose children are the
/// per-dependency-graph hash directories.
fn pkg_version_dir(store_dir: &Path, name: &str, version: &str) -> PathBuf {
    gvs_root(store_dir).join(name).join(version)
}

/// Sorted hash-directory names under a `<name>/<version>` directory.
///
/// Upstream reads these with `fs.readdirSync` and asserts on the count and
/// on identity across installs; sorting keeps the comparison stable.
fn hash_dirs(pkg_version_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(pkg_version_dir)
        .unwrap_or_else(|err| panic!("read hash dirs under {pkg_version_dir:?}: {err}"))
        .map(|entry| entry.expect("read hash dir entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The single hash directory under `<name>/<version>`, asserting there is
/// exactly one — upstream's `expect(files).toHaveLength(1)`.
fn sole_hash_dir(pkg_version_dir: &Path) -> PathBuf {
    let hashes = hash_dirs(pkg_version_dir);
    assert_eq!(
        hashes.len(),
        1,
        "expected exactly one hash directory under {pkg_version_dir:?}, got {hashes:?}",
    );
    pkg_version_dir.join(&hashes[0])
}

/// Replace a file the importer materialized with garbage, standing in
/// for a slot left half-written by a crashed install.
///
/// Unlinking before writing matters: when the store and the slot sit on
/// one filesystem the importer hardlinks, so truncating the slot's copy
/// in place would rewrite the store's content-addressed file too — and
/// then no re-import could ever restore the original bytes.
fn corrupt_pristine_file(path: &Path) {
    fs::remove_file(path).unwrap_or_else(|err| panic!("unlink {path:?}: {err}"));
    fs::write(path, "{}").unwrap_or_else(|err| panic!("corrupt {path:?}: {err}"));
}

/// `<hash>/node_modules/<name>` — where the package's files actually live.
fn pkg_in_slot(hash_dir: &Path, name: &str) -> PathBuf {
    hash_dir.join("node_modules").join(name)
}

/// Rewrite `pnpm-workspace.yaml` as the harness's `storeDir` / `cacheDir`
/// plus `enableGlobalVirtualStore: true` and `extra_yaml`.
///
/// [`enable_gvs_in_workspace_yaml`] asserts it is flipping the harness's
/// `enableGlobalVirtualStore: false` line, so it can only be called once.
/// These tests re-set `allowBuilds` between installs, so they need a form
/// that is idempotent.
fn set_gvs_workspace_yaml(workspace: &Path, extra_yaml: &str) {
    let mut yaml = harness_store_and_cache_yaml(workspace);
    yaml.push_str("enableGlobalVirtualStore: true\n");
    yaml.push_str(extra_yaml);
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");
}

/// The harness's `storeDir` / `cacheDir` lines on their own, for a test
/// that writes its own virtual-store setting on top.
fn harness_store_and_cache_yaml(workspace: &Path) -> String {
    let existing = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    let yaml: String = existing
        .lines()
        .filter(|line| line.starts_with("storeDir:") || line.starts_with("cacheDir:"))
        .fold(String::new(), |mut acc, line| {
            acc.push_str(line);
            acc.push('\n');
            acc
        });
    assert!(
        !yaml.is_empty(),
        "expected the `storeDir` / `cacheDir` keys written by \
         `CommandTempCwd::add_mocked_registry` — has the helper changed?",
    );
    yaml
}

fn write_manifest(workspace: &Path, deps: &serde_json::Value) {
    let manifest = serde_json::json!({ "dependencies": deps });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// A fresh `Command` for the pacquet binary — `assert_cmd`'s `Command` is
/// single-use, so every sequential install needs its own.
fn pacquet(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn read_modules_manifest(workspace: &Path) -> pnpm_modules_yaml::Modules {
    pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(
        &workspace.join("node_modules"),
    )
    .expect("read .modules.yaml")
    .expect(".modules.yaml must exist after an install")
}

/// Render an `allowBuilds:` block for [`set_gvs_workspace_yaml`]'s
/// `extra_yaml`. An empty slice yields `allowBuilds: {}` — upstream's
/// `allowBuilds: {}`, which is materially different from omitting the key
/// because it pins "nothing may build" rather than "no opinion".
fn allow_builds_yaml(entries: &[(&str, bool)]) -> String {
    if entries.is_empty() {
        return "allowBuilds: {}\n".to_string();
    }
    let mut yaml = String::from("allowBuilds:\n");
    for (spec, value) in entries {
        writeln!(yaml, "  '{spec}': {value}").expect("format an allowBuilds entry");
    }
    yaml
}

/// TS: `using a global virtual store` (`globalVirtualStore.ts:21`), which
/// is also the CLI-level `pnpm/test/install/globalVirtualStore.ts:11`.
/// Both halves of the upstream test: a fresh install populates the GVS and
/// the private hoist, then wiping `node_modules` *and* the GVS and
/// reinstalling frozen rebuilds the identical layout.
#[test]
fn using_a_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "privateHoistPattern:\n  - '*'\n");
    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    let assert_layout = |phase: &str| {
        assert!(
            workspace
                .join(
                    "node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json"
                )
                .exists(),
            "{phase}: the transitive dep must be privately hoisted",
        );
        assert!(
            workspace.join("node_modules/.pnpm/lock.yaml").exists(),
            "{phase}: the current lockfile must be written",
        );
        let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
        let hash_dir = sole_hash_dir(&version_dir);
        assert!(
            pkg_in_slot(&hash_dir, "@pnpm.e2e/pkg-with-1-dep").join("package.json").exists(),
            "{phase}: the package must be materialized in its GVS slot",
        );
        assert!(
            pkg_in_slot(&hash_dir, "@pnpm.e2e/dep-of-pkg-with-1-dep").join("package.json").exists(),
            "{phase}: the slot's own node_modules must carry the transitive dep",
        );
    };

    eprintln!("Fresh install with GVS enabled...");
    pacquet(&workspace).with_arg("install").assert().success();
    assert_layout("fresh install");

    eprintln!("Wiping node_modules and the whole GVS, then reinstalling frozen...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_dir_all(gvs_root(&store_dir)).expect("remove the GVS root");
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_layout("frozen reinstall from a cold GVS");

    drop((root, mock_instance));
}

/// TS: `reinstall from warm global virtual store after deleting
/// node_modules` (`globalVirtualStore.ts:63`) and the CLI-level `warm GVS
/// reinstall skips internal linking` (`pnpm/test/install/globalVirtualStore.ts:80`).
///
/// Upstream additionally wraps `storeController.fetchPackage` to assert it
/// is never called. Pacquet's CLI exposes no such seam, so the port
/// asserts the observable consequence instead: the warm slot is reused
/// rather than rebuilt beside a second hash directory, and the project
/// tree — including `.bin` — is fully restored from it.
#[test]
fn reinstall_from_warm_global_virtual_store_after_deleting_node_modules() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "privateHoistPattern:\n  - '*'\n");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "@pnpm.e2e/hello-world-js-bin": "1.0.0",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }),
    );

    eprintln!("First install — warms the GVS...");
    pacquet(&workspace).with_arg("install").assert().success();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let hashes_before = hash_dirs(&version_dir);

    eprintln!("Deleting node_modules only — the GVS stays warm...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    assert!(gvs_root(&store_dir).is_dir(), "the GVS must survive the node_modules wipe");

    eprintln!("Frozen reinstall — must reattach from the warm GVS...");
    let output = pacquet(&workspace)
        .with_args(["install", "--frozen-lockfile", "--reporter=ndjson"])
        .output()
        .expect("run the frozen reinstall");
    assert_success(&output);
    let added: Vec<u64> = ndjson_records(&output)
        .iter()
        .filter(|record| record["name"] == "pnpm:stats")
        .filter_map(|record| record["added"].as_u64())
        .collect();
    assert_eq!(
        added,
        vec![0],
        "a wiped node_modules loses the current lockfile, but the content-addressed \
         GVS slots are still authoritative, so nothing may be re-materialized",
    );

    assert_eq!(
        hash_dirs(&version_dir),
        hashes_before,
        "a warm reinstall must reuse the existing slot, not materialize a second one",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep/package.json").exists(),
        "the direct dep must be relinked into the project",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin/package.json").exists(),
        "every direct dep must be relinked, not just the first",
    );
    assert!(
        workspace
            .join("node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep")
            .try_exists()
            .expect("stat the hoisted dep"),
        "the private hoist must be rebuilt from the warm GVS",
    );
    assert!(
        workspace.join("node_modules/.bin/hello-world-js-bin").exists(),
        "bins must be relinked after a warm reinstall",
    );
    assert!(
        workspace.join("node_modules/.pnpm/lock.yaml").exists(),
        "the current lockfile must be rewritten",
    );

    drop((root, mock_instance));
}

/// A global-virtual-store slot is filled in place, so an interrupted
/// import leaves a directory holding only part of the package. The
/// completion marker (`package.json`, written last) is what tells the
/// two apart, and a reinstall has to repair a slot that lacks it rather
/// than accept the directory as already materialized.
#[test]
fn a_slot_left_incomplete_by_an_interrupted_import_is_repaired() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "");
    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    pacquet(&workspace).with_arg("install").assert().success();

    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    let pkg = pkg_in_slot(&sole_hash_dir(&version_dir), "@pnpm.e2e/pkg-with-1-dep");
    let marker = pkg.join("package.json");
    let pristine_marker = fs::read_to_string(&marker).expect("read the completion marker");

    eprintln!("Simulating an import that died before writing the marker...");
    fs::remove_file(&marker).expect("remove the completion marker");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert_eq!(
        fs::read_to_string(&marker).expect("read the repaired marker"),
        pristine_marker,
        "the reinstall must finish the interrupted import at {marker:?}",
    );

    drop((root, mock_instance));
}

#[test]
fn concurrent_installs_sharing_a_gvs_do_not_fail_while_linking_bins() {
    const WORKERS: usize = 8;
    const REPETITIONS: usize = 20;
    const PARENT: &str = "@pnpm.e2e/hello-world-js-bin-parent";

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "");
    write_manifest(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    pacquet(&workspace).with_arg("install").assert().success();

    let fixture_files =
        ["package.json", "pnpm-workspace.yaml", ".npmrc", "pnpm-lock.yaml"].map(|name| {
            let bytes = fs::read(workspace.join(name)).expect("read concurrent-install fixture");
            (name, bytes)
        });
    // Siblings of `workspace`, not children of it: the harness writes
    // `storeDir` / `cacheDir` as `../pacquet-store` / `../pacquet-cache`, so
    // only at this depth do all the workers resolve to the one store — and
    // with it the one GVS whose slots they race over. Nested any deeper they
    // would each get a private store and the test would pass vacuously.
    let worker_dirs = (0..WORKERS)
        .map(|worker| {
            let dir = root.path().join(format!("concurrent-gvs-{worker}"));
            fs::create_dir(&dir).expect("create concurrent-install workspace");
            for (name, bytes) in &fixture_files {
                fs::write(dir.join(*name), bytes).expect("write concurrent-install fixture");
            }
            dir
        })
        .collect::<Vec<_>>();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove priming node_modules");
    fs::remove_dir_all(gvs_root(&store_dir)).expect("remove priming GVS");

    for repetition in 1..=REPETITIONS {
        let children = worker_dirs
            .iter()
            .enumerate()
            .map(|(worker, dir)| {
                let mut command = pacquet(dir);
                command
                    .args(["install", "--frozen-lockfile"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped());
                let child = command.spawn().expect("spawn concurrent GVS install");
                (worker, child)
            })
            .collect::<Vec<_>>();

        for (worker, child) in children {
            let output = child.wait_with_output().expect("wait for concurrent GVS install");
            assert!(
                output.status.success(),
                "worker {worker} failed in repetition {repetition}: {}",
                String::from_utf8_lossy(&output.stderr),
            );
        }

        let version_dir = pkg_version_dir(&store_dir, PARENT, "1.0.0");
        let hash_dir = sole_hash_dir(&version_dir);
        let shared_bin =
            pkg_in_slot(&hash_dir, PARENT).join("node_modules/.bin/hello-world-js-bin");
        assert!(shared_bin.exists(), "the shared GVS bin must remain materialized");

        if repetition < REPETITIONS {
            for dir in &worker_dirs {
                fs::remove_dir_all(dir.join("node_modules"))
                    .expect("remove worker node_modules before the next repetition");
            }
            fs::remove_dir_all(gvs_root(&store_dir)).expect("reset GVS before the next repetition");
        }
    }

    drop((root, mock_instance));
}

/// TS: `modules are correctly updated when using a global virtual store`
/// (`globalVirtualStore.ts:107`). Bumping one dependency's version must
/// materialize a slot for the new version.
#[test]
fn modules_are_correctly_updated_when_using_a_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            "@pnpm.e2e/peer-c": "1.0.0",
        }),
    );

    eprintln!("Installing with peer-c 1.0.0...");
    pacquet(&workspace).with_arg("install").assert().success();

    eprintln!("Bumping peer-c to 2.0.0 and reinstalling...");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            "@pnpm.e2e/peer-c": "2.0.0",
        }),
    );
    pacquet(&workspace).with_arg("install").assert().success();

    assert!(
        workspace.join("node_modules/.pnpm/lock.yaml").exists(),
        "the current lockfile must be rewritten",
    );
    let version_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/peer-c", "2.0.0");
    let hash_dir = sole_hash_dir(&version_dir);
    assert!(
        pkg_in_slot(&hash_dir, "@pnpm.e2e/peer-c").join("package.json").exists(),
        "the newly-resolved version must be materialized in its own GVS slot",
    );

    drop((root, mock_instance));
}

/// TS: `local directory dependency works with global virtual store`
/// (`globalVirtualStore.ts`).
///
/// The lockfile records no version for a directory snapshot while the
/// resolver reads one off the manifest, so both install paths have to
/// agree on the anchored `directory` segment or the reinstall would
/// relocate the package. Upstream additionally crashed here — see
/// <https://github.com/pnpm/pnpm/issues/13335>.
#[test]
fn local_directory_dependency_works_with_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "");
    write_manifest(&workspace, &serde_json::json!({ "dep": "file:dep" }));
    fs::create_dir_all(workspace.join("dep")).expect("mkdir dep");
    fs::write(
        workspace.join("dep/package.json"),
        serde_json::json!({ "name": "dep", "version": "1.0.0" }).to_string(),
    )
    .expect("write dep/package.json");

    pacquet(&workspace).with_arg("install").assert().success();

    let version_dir = pkg_version_dir(&store_dir, "@/dep", "directory");
    let slot_after_install = sole_hash_dir(&version_dir);
    assert!(
        pkg_in_slot(&slot_after_install, "dep").join("package.json").exists(),
        "the local directory dependency must be materialized in its GVS slot",
    );

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert_eq!(
        sole_hash_dir(&version_dir),
        slot_after_install,
        "the frozen reinstall must land on the slot the first install created",
    );

    drop((root, mock_instance));
}

/// TS: `injected local packages work with global virtual store`
/// (`globalVirtualStore.ts:461`).
///
/// The materialization half is already pinned by
/// `injected_workspace_dep_with_dedupe_off_materialises_under_gvs` in
/// `dedupe_injected_deps.rs`; this port covers the half that one does not
/// — `.modules.yaml.injectedDeps` pointing into the GVS.
#[test]
fn injected_local_packages_work_with_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");
    set_gvs_workspace_yaml(&workspace, "packages:\n  - 'project-*'\ndedupeInjectedDeps: false\n");

    fs::create_dir_all(workspace.join("project-1")).expect("mkdir project-1");
    fs::write(
        workspace.join("project-1/package.json"),
        serde_json::json!({
            "name": "project-1",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write project-1/package.json");
    fs::write(workspace.join("project-1/foo.js"), "").expect("write project-1/foo.js");

    fs::create_dir_all(workspace.join("project-2")).expect("mkdir project-2");
    fs::write(
        workspace.join("project-2/package.json"),
        serde_json::json!({
            "name": "project-2",
            "version": "1.0.0",
            "dependencies": { "project-1": "workspace:1.0.0" },
            "dependenciesMeta": { "project-1": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write project-2/package.json");

    pacquet(&workspace).with_arg("install").assert().success();

    assert!(
        workspace.join("project-2/node_modules/project-1").exists(),
        "project-2 must have the injected workspace package installed",
    );

    let injected_deps = read_modules_manifest(&workspace)
        .injected_deps
        .expect(".modules.yaml must record injectedDeps under GVS");
    let locations =
        injected_deps.get("project-1").expect("injectedDeps must have an entry for project-1");
    assert!(!locations.is_empty(), "the injectedDeps entry must list at least one location");

    let gvs_root = gvs_root(&store_dir);
    let location = Path::new(&locations[0]);
    // `.modules.yaml` stores injected-dep locations relative to the
    // project root, matching pnpm — upstream's assertion joins the
    // recorded value straight onto the test's cwd.
    let resolved =
        if location.is_absolute() { location.to_path_buf() } else { workspace.join(location) };
    let resolved = dunce::canonicalize(&resolved)
        .unwrap_or_else(|err| panic!("canonicalize injected dep location {resolved:?}: {err}"));
    let gvs_root = dunce::canonicalize(&gvs_root).expect("canonicalize the GVS root");
    assert!(
        resolved.starts_with(&gvs_root),
        "the injected dep must be materialized inside the GVS ({gvs_root:?}), got {resolved:?}",
    );
    assert!(
        resolved.join("foo.js").exists(),
        "the injected copy must carry the source project's files",
    );

    drop((root, mock_instance));
}

/// The three negatives every `virtualStoreOnly` install shares: no
/// importer symlinks, no hoisted packages, no `.bin`.
fn assert_no_post_import_linking(workspace: &Path) {
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists(),
        "importer-level symlinks must not be created",
    );
    assert!(
        !workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep").exists(),
        "nothing must be hoisted",
    );
    assert!(!workspace.join("node_modules/.bin").exists(), "no bins must be linked");
}

/// The GVS hash of a package inside a dependency cycle depends on the
/// order the hasher walks the lockfile, so an install that walked the
/// parsed `HashMap`s directly re-derived a *different* slot for the
/// cycle on some fraction of its runs — and re-imported the packages
/// that landed there. `pnpm install --frozen-lockfile` over an
/// unchanged project must reuse the slots the previous run
/// materialized ([pnpm/pnpm#13316](https://github.com/pnpm/pnpm/issues/13316)).
#[test]
fn repeat_installs_reuse_the_slots_of_circular_dependencies() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/circular-deps-1-of-2": "1.0.2" }));
    set_gvs_workspace_yaml(&workspace, "");
    pacquet(&workspace).with_arg("install").assert().success();

    let version_dirs = [
        pkg_version_dir(&store_dir, "@pnpm.e2e/circular-deps-1-of-2", "1.0.2"),
        pkg_version_dir(&store_dir, "@pnpm.e2e/circular-deps-2-of-2", "1.0.2"),
    ];
    let slots_after_first: Vec<Vec<String>> =
        version_dirs.iter().map(|dir| hash_dirs(dir)).collect();

    // Each repeat is a fresh process, and therefore a fresh set of
    // `HashMap` iteration orders.
    for _ in 0..3 {
        pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    }

    for (version_dir, expected) in version_dirs.iter().zip(&slots_after_first) {
        assert_eq!(
            &hash_dirs(version_dir),
            expected,
            "a repeat install moved {version_dir:?} to a new slot",
        );
    }

    drop((root, mock_instance));
}

/// A `link:` dependency inside a slot has to be materialized as a
/// symlink to the directory the lockfile names.
///
/// The dependency is a peer resolved by a workspace sibling, so the
/// lockfile records `@pnpm.e2e/peer-a: link:packages/peer-a` on the
/// `@pnpm.e2e/abc` snapshot (see the `exclude_links_from_lockfile`
/// suite for the resolution half). Nothing used to create that link:
/// project-locally the omission is invisible, because the slot sits
/// under the importer's `node_modules` and Node's upward walk finds the
/// importer's own copy of the peer. A GVS slot lives in the shared
/// store where no such walk exists, so without the link the peer is
/// simply missing from the package that declared it.
#[test]
fn link_dep_in_a_slot_is_symlinked_to_its_target() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "packages:\n  - 'packages/*'\n");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    let write_project = |relative_dir: &str, manifest: &serde_json::Value| {
        let project_dir = workspace.join(relative_dir);
        fs::create_dir_all(&project_dir).expect("create project directory");
        fs::write(project_dir.join("package.json"), manifest.to_string())
            .expect("write project manifest");
    };
    write_project(
        "packages/app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/abc": "1.0.0",
                "@pnpm.e2e/peer-a": "workspace:*",
                "@pnpm.e2e/peer-b": "1.0.0",
                "@pnpm.e2e/peer-c": "1.0.0",
            },
        }),
    );
    write_project(
        "packages/peer-a",
        &serde_json::json!({ "name": "@pnpm.e2e/peer-a", "version": "1.0.0" }),
    );

    pacquet(&workspace).with_arg("install").assert().success();

    let hash_dir = sole_hash_dir(&pkg_version_dir(&store_dir, "@pnpm.e2e/abc", "1.0.0"));
    let linked_peer = pkg_in_slot(&hash_dir, "@pnpm.e2e/peer-a");
    assert!(
        is_symlink_or_junction(&linked_peer).unwrap_or(false),
        "the linked peer must be materialized inside the slot at {linked_peer:?}",
    );
    assert_eq!(
        fs::canonicalize(&linked_peer).expect("resolve the linked peer"),
        fs::canonicalize(workspace.join("packages/peer-a")).expect("resolve the link target"),
        "the slot's link must point at the workspace package the lockfile names",
    );

    drop((root, mock_instance));
}

#[test]
fn no_optional_excludes_an_optional_link_dep_from_a_slot() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "packages:\n  - 'packages/*'\n");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    let app_dir = workspace.join("packages/app");
    let peer_dir = workspace.join("packages/peer-c");
    fs::create_dir_all(&app_dir).expect("create app directory");
    fs::create_dir_all(&peer_dir).expect("create peer directory");
    fs::write(
        app_dir.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/abc-optional-peers": "1.0.0",
                "@pnpm.e2e/peer-c": "workspace:*",
            },
        })
        .to_string(),
    )
    .expect("write app manifest");
    fs::write(
        peer_dir.join("package.json"),
        serde_json::json!({ "name": "@pnpm.e2e/peer-c", "version": "1.0.0" }).to_string(),
    )
    .expect("write peer manifest");

    pacquet(&workspace).with_arg("install").assert().success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let snapshots = snapshot_entries(&lockfile, "@pnpm.e2e/abc-optional-peers");
    let (_, snapshot) = snapshots.as_slice().first().expect("abc optional-peers snapshot");
    let peer_name = "@pnpm.e2e/peer-c".parse().expect("parse peer name");
    assert!(
        snapshot
            .optional_dependencies
            .as_ref()
            .and_then(|dependencies| dependencies.get(&peer_name))
            .is_some_and(|dep_ref| dep_ref.as_link_target().is_some()),
        "the fixture must exercise an optional link edge: {snapshot:?}",
    );

    let hash_dir =
        sole_hash_dir(&pkg_version_dir(&store_dir, "@pnpm.e2e/abc-optional-peers", "1.0.0"));
    let linked_peer = pkg_in_slot(&hash_dir, "@pnpm.e2e/peer-c");
    assert!(
        is_symlink_or_junction(&linked_peer).unwrap_or(false),
        "the first install must materialize the optional link at {linked_peer:?}",
    );

    pacquet(&workspace).with_args(["install", "--no-optional"]).assert().success();

    assert!(
        matches!(
            fs::symlink_metadata(&linked_peer),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ),
        "--no-optional must leave the optional link out of the slot at {linked_peer:?}",
    );

    pacquet(&workspace).with_arg("install").assert().success();
    assert!(
        is_symlink_or_junction(&linked_peer).unwrap_or(false),
        "re-enabling optional dependencies must restore the link at {linked_peer:?}",
    );

    drop((root, mock_instance));
}

/// A slot path is content-addressed, so serving an install the previous
/// run's derived slot-path map would relocate packages. The install that
/// adds a dependency is the sharp case: the lockfile on disk is still
/// the one without it, so a map keyed on that file alone would be handed
/// to an install whose dependency set has grown, and the added package,
/// absent from the map, would be materialized at the flat-named
/// `<name>@<version>` fallback outside the global virtual store.
#[test]
fn adding_a_dependency_over_a_warm_layout_cache_still_hashes_its_slot() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    set_gvs_workspace_yaml(&workspace, "");
    write_manifest(&workspace, &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));

    eprintln!("Installing twice, which is what warms the derived-layout cache...");
    pacquet(&workspace).with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet(&workspace).with_arg("install").assert().success();

    let layout_cache = workspace.join(
        fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
            .expect("read pnpm-workspace.yaml")
            .lines()
            .find_map(|line| line.strip_prefix("cacheDir: ").map(ToOwned::to_owned))
            .expect("the harness writes a cacheDir"),
    );
    assert!(
        layout_cache.join("gvs-layout").is_dir(),
        "the layout cache must be warm at {layout_cache:?}, or this test proves nothing",
    );

    eprintln!("Adding a second dependency...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    write_manifest(
        &workspace,
        &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0", "@pnpm.e2e/foo": "100.0.0" }),
    );
    pacquet(&workspace).with_arg("install").assert().success();

    let added = fs::read_link(workspace.join("node_modules/@pnpm.e2e/foo"))
        .expect("read the added dependency symlink");
    let hash_dir = pkg_version_dir(&store_dir, "@pnpm.e2e/foo", "100.0.0");
    assert!(
        added.ends_with(
            sole_hash_dir(&hash_dir)
                .join("node_modules")
                .join("@pnpm.e2e/foo")
                .strip_prefix(hash_dir.parent().and_then(Path::parent).expect("<links>/<scope>"),)
                .expect("the hash dir sits under the links root"),
        ),
        "the added dependency must be linked from its hashed slot, not from {added:?}",
    );

    drop((root, mock_instance));
}

mod builds;

mod layout;
