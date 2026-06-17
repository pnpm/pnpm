//! `--lockfile-only` coverage for `pacquet install`.
//!
//! Ports pnpm's
//! [`installing/deps-installer/test/install/lockfileOnly.ts`](https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/deps-installer/test/install/lockfileOnly.ts):
//! resolving with `lockfileOnly` writes `pnpm-lock.yaml` (direct and
//! transitive deps) without fetching any tarball into the store or
//! creating `node_modules`, and a repeat run keeps that property. The
//! `--frozen-lockfile --lockfile-only` combination still validates the
//! on-disk lockfile against the manifest and fails when it is stale.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::get_all_files,
};
use std::{fs, path::Path, process::Command};

/// A fresh `pacquet` command rooted at `workspace`. `std::process::Command`
/// isn't `Clone` and each invocation consumes the builder, so tests that
/// run pacquet more than once rebuild it here.
fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// Content-addressable tarball blobs under the store (`v11/files/...`).
/// `--lockfile-only` writes none; the empty `v11/index.db` the writer
/// task always creates is excluded so the assertion targets fetched
/// package content, not store scaffolding.
fn cas_blobs(store_dir: &Path) -> Vec<String> {
    get_all_files(store_dir)
        .into_iter()
        .filter(|path| {
            Path::new(path).components().any(|component| component.as_os_str() == "files")
        })
        .collect()
}

/// `pacquet install --lockfile-only` resolves the graph, writes
/// `pnpm-lock.yaml` (direct + transitive deps), and stops: no tarball
/// reaches the store, and no `node_modules` is created. A second
/// `--lockfile-only` run against the now-present lockfile keeps both
/// properties.
#[test]
fn writes_lockfile_without_downloading_or_linking() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep"),
        "lockfile must record the direct dependency:\n{lockfile}",
    );
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep@100.0.0"),
        "lockfile must pin the resolved direct-dep version:\n{lockfile}",
    );
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@"),
        "lockfile must record the transitive dependency:\n{lockfile}",
    );

    assert!(
        !workspace.join("node_modules").exists(),
        "node_modules must not be created by --lockfile-only",
    );
    assert!(
        cas_blobs(&store_dir).is_empty(),
        "the store must hold no tarball CAS blob — nothing is fetched by --lockfile-only; got:\n{:#?}",
        cas_blobs(&store_dir),
    );

    // Repeat run: still resolve-and-write only, nothing materialized.
    pacquet_at(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    assert!(
        !workspace.join("node_modules").exists(),
        "node_modules must stay absent on a repeat --lockfile-only run",
    );
    assert!(
        cas_blobs(&store_dir).is_empty(),
        "the store must hold no tarball CAS blob on a repeat --lockfile-only run",
    );

    // A subsequent ordinary install materializes from the lockfile the
    // --lockfile-only run produced, proving it left a usable state.
    pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0").exists(),
        "a normal install after --lockfile-only must materialize the virtual store",
    );

    drop((root, mock_instance));
}

/// `--frozen-lockfile --lockfile-only` keeps frozen semantics: it
/// validates the on-disk `pnpm-lock.yaml` against the manifest and
/// fails when the manifest has drifted, never rewriting the lockfile.
#[test]
fn frozen_lockfile_only_rejects_a_stale_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    // Drift the manifest away from the locked specifier.
    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "is-positive": "2.0.0" } }).to_string(),
    )
    .expect("rewrite package.json");

    let output = pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--lockfile-only"])
        .output()
        .expect("spawn pacquet install");

    assert!(
        !output.status.success(),
        "frozen + lockfile-only must reject a stale lockfile (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    // miette wraps the long "is not up to date" sentence across lines,
    // so assert on the stable diagnostic code instead of the prose.
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("outdated_lockfile"),
        "stderr must name the outdated-lockfile diagnostic; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// `--frozen-lockfile --lockfile-only` against an up-to-date lockfile
/// succeeds via the headless dispatch: the freshness gate passes, the
/// lockfile is re-persisted, and nothing is materialized. Pins the
/// explicit-frozen happy path that shares a branch with the auto-frozen
/// (`preferFrozenLockfile`) repeat run covered above.
#[test]
fn frozen_lockfile_only_succeeds_without_materializing_when_fresh() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    // Seed an up-to-date lockfile with a plain lockfile-only run.
    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    assert!(lockfile_path.exists(), "the seeding run must write pnpm-lock.yaml");

    // Headless lockfile-only: lockfile matches the manifest, so the
    // freshness gate passes and the run returns after re-persisting the
    // lockfile, without creating node_modules or fetching a tarball.
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--lockfile-only"])
        .assert()
        .success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("is-positive@1.0.0"),
        "the lockfile must still pin is-positive@1.0.0:\n{lockfile}",
    );
    assert!(
        !workspace.join("node_modules").exists(),
        "node_modules must not be created by --frozen-lockfile --lockfile-only",
    );
    assert!(
        cas_blobs(&store_dir).is_empty(),
        "the store must hold no tarball CAS blob under --frozen-lockfile --lockfile-only",
    );

    drop((root, mock_instance));
}

/// `--lockfile-only` together with `lockfile: false` (pnpm's
/// `useLockfile: false`) is a config conflict — the only output the
/// flag produces is the lockfile, which `lockfile: false` disables.
/// Ports pnpm's
/// [`lockfile.ts` "fail when installing with useLockfile: false and lockfileOnly: true"](https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/deps-installer/test/lockfile.ts#L727-L736).
#[test]
fn lockfile_false_with_lockfile_only_is_a_config_conflict() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("lockfile: false\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    let output =
        pacquet.with_args(["install", "--lockfile-only"]).output().expect("spawn pacquet install");

    assert!(
        !output.status.success(),
        "lockfile: false + --lockfile-only must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE"),
        "stderr must name the upstream config-conflict code; got:\n{stderr}",
    );
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "no pnpm-lock.yaml must be written on the rejected install",
    );

    drop((root, mock_instance));
}

/// In a workspace, a fresh `--lockfile-only` run records every
/// importer, and a later run after a new project is added updates the
/// lockfile to include it — all without materializing `node_modules`.
/// Ports pnpm's
/// [`lockfile.ts` "update the lockfile when a new project is added to the workspace and lockfile-only installation is used"](https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/deps-installer/test/lockfile.ts#L1564-L1610).
#[test]
fn lockfile_only_updates_importers_when_a_project_is_added() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/project-1")).expect("mkdir project-1");
    fs::write(
        workspace.join("packages/project-1/package.json"),
        serde_json::json!({
            "name": "project-1",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write project-1 package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("packages/project-1:"),
        "lockfile must record the project-1 importer:\n{lockfile}",
    );
    assert!(
        !workspace.join("node_modules").exists(),
        "node_modules must not be created by --lockfile-only",
    );

    // Add a second project, re-run lockfile-only, and confirm both
    // importers are recorded. `--no-prefer-frozen-lockfile` forces the
    // fresh-resolve path: pacquet's auto-frozen freshness gate
    // (`check_lockfile_freshness`) only validates the root importer
    // today, so it wouldn't notice a newly-added sibling and would
    // otherwise short-circuit to the frozen path. Re-resolving is what
    // pnpm's `mutateModules` does unconditionally in the upstream test.
    fs::create_dir_all(workspace.join("packages/project-2")).expect("mkdir project-2");
    fs::write(
        workspace.join("packages/project-2/package.json"),
        serde_json::json!({ "name": "project-2", "version": "1.0.0" }).to_string(),
    )
    .expect("write project-2 package.json");

    pacquet_at(&workspace)
        .with_args(["install", "--lockfile-only", "--no-prefer-frozen-lockfile"])
        .assert()
        .success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("re-read pnpm-lock.yaml");
    assert!(
        lockfile.contains("packages/project-1:"),
        "lockfile must keep the project-1 importer:\n{lockfile}",
    );
    assert!(
        lockfile.contains("packages/project-2:"),
        "lockfile must record the newly added project-2 importer:\n{lockfile}",
    );

    drop((root, mock_instance));
}
