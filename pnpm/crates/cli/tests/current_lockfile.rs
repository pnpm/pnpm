//! Ports of the upstream "write and update the current lockfile"
//! suite — `node_modules/.pnpm/lock.yaml`, the record of what the last
//! install actually materialized, as opposed to `pnpm-lock.yaml`'s
//! record of what it resolved.

pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

const CURRENT_LOCKFILE: &str = "node_modules/.pnpm/lock.yaml";

fn rerun(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn package_names(lockfile: &pacquet_lockfile::Lockfile) -> Vec<String> {
    let mut names: Vec<String> = lockfile
        .packages
        .iter()
        .flat_map(|packages| packages.keys())
        .map(ToString::to_string)
        .collect();
    names.sort();
    names
}

/// TS: `installing a simple project` (`deps-restorer/test/index.ts:54`),
/// the current-lockfile half. A frozen install materializes from
/// `pnpm-lock.yaml` and leaves behind a current lockfile naming what it
/// put on disk.
#[test]
fn a_frozen_install_writes_the_current_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    assert!(
        !workspace.join(CURRENT_LOCKFILE).exists(),
        "--lockfile-only materializes nothing, so there is nothing to record",
    );

    rerun(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert_eq!(
        package_names(&read_current_lockfile(&workspace)),
        ["@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0", "@pnpm.e2e/pkg-with-1-dep@100.0.0"],
    );

    drop((root, mock_instance));
}

/// TS: `dependency should not be added to current lockfile if it was not
/// built successfully during headless install`
/// (`deps-installer/test/install/lifecycleScripts.ts:547`). The current
/// lockfile is a record of completed work, so a failed build must leave
/// none behind.
#[test]
fn a_failed_build_writes_no_current_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/failing-postinstall": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "allowBuilds:\n  '@pnpm.e2e/failing-postinstall': true\n",
    )
    .expect("write pnpm-workspace.yaml");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    let output = rerun(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run the frozen install");
    eprintln!(
        "frozen install output:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.status.success(), "a failing postinstall must fail the install");

    assert!(
        !workspace.join(CURRENT_LOCKFILE).exists(),
        "a failed build must not leave a current lockfile claiming the install finished",
    );

    drop((root, mock_instance));
}

/// TS: `use current pnpm-lock.yaml as initial wanted one, when wanted was
/// removed` (`deps-installer/test/lockfile.ts:1007`). Deleting
/// `pnpm-lock.yaml` must not re-resolve from scratch — the current
/// lockfile stands in, so the already-installed versions are preserved
/// and the wanted lockfile is regenerated from them.
#[test]
fn a_deleted_wanted_lockfile_is_regenerated_from_the_current_one() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pkg-with-1-dep": "^100.0.0",
            "@pnpm.e2e/foo": "^100.0.0",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    let wanted_path = workspace.join("pnpm-lock.yaml");
    let wanted = fs::read_to_string(&wanted_path).expect("read pnpm-lock.yaml");
    let current = package_names(&read_current_lockfile(&workspace));

    fs::remove_file(&wanted_path).expect("remove pnpm-lock.yaml");
    // The harness writes a `pnpm-workspace.yaml` for storeDir/cacheDir, so
    // without this the freshness fast path treats the run as a workspace
    // install and never reaches the synthesis branch.
    fs::remove_file(workspace.join("node_modules/.pnpm-workspace-state-v1.json"))
        .expect("remove the workspace state file");

    rerun(&workspace).with_arg("install").assert().success();

    assert_eq!(
        fs::read_to_string(&wanted_path).expect("read the regenerated pnpm-lock.yaml"),
        wanted,
        "the wanted lockfile must be rebuilt from the current one, not re-resolved",
    );
    assert_eq!(package_names(&read_current_lockfile(&workspace)), current);

    drop((root, mock_instance));
}

/// TS: `a lockfile with duplicate keys causes an exception, when
/// frozenLockfile is true` (`deps-installer/test/lockfile.ts:1324`). A
/// duplicated mapping key is a broken file, not a mergeable one, and a
/// frozen install must refuse it rather than silently take one branch.
#[test]
fn a_wanted_lockfile_with_duplicate_keys_fails_a_frozen_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    fs::write(&lockfile_path, format!("{lockfile}\nlockfileVersion: '9.0'\n"))
        .expect("duplicate a top-level key");

    let output = rerun(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run the frozen install");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprintln!("frozen install output:\n{combined}");
    assert!(!output.status.success(), "a duplicated mapping key must fail the install");
    assert!(
        combined.contains("ERR_PNPM_BROKEN_LOCKFILE"),
        "expected the broken-lockfile error code; got:\n{combined}",
    );

    drop((root, mock_instance));
}

/// TS: `installing with package manifest ignored` and its prod-only
/// sibling (`deps-restorer/test/index.ts:165`, `:189`). The current
/// lockfile records what was materialized, so excluding a dependency
/// group drops it from `packages:` even though the wanted lockfile still
/// carries it. The dev-only variant (`:213`) is covered by
/// `headless_install_include_filtering_excludes_production_group` in
/// `optional_dependencies.rs`.
#[test]
fn the_current_lockfile_is_filtered_to_the_installed_groups() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        "devDependencies": { "@pnpm.e2e/bar": "100.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    assert_eq!(
        package_names(&read_current_lockfile(&workspace)),
        ["@pnpm.e2e/bar@100.0.0", "@pnpm.e2e/foo@100.0.0"],
    );

    rerun(&workspace).with_args(["install", "--prod"]).assert().success();
    assert_eq!(
        package_names(&read_current_lockfile(&workspace)),
        ["@pnpm.e2e/foo@100.0.0"],
        "a prod-only install must drop the dev dependency it did not materialize",
    );

    rerun(&workspace).with_arg("install").assert().success();
    assert_eq!(
        package_names(&read_current_lockfile(&workspace)),
        ["@pnpm.e2e/bar@100.0.0", "@pnpm.e2e/foo@100.0.0"],
        "restoring the dev group must bring its package back",
    );

    drop((root, mock_instance));
}

/// TS: `packages are updated in node_modules, when packageImportMethod is
/// set to copy and modules manifest and current lockfile are incorrect`
/// (`deps-installer/test/packageImportMethods.ts:31`). Restoring stale
/// state files must not convince the install that `node_modules` is
/// already correct — the tree is re-derived from the wanted lockfile.
#[test]
fn stale_state_files_do_not_stop_node_modules_from_being_repaired() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let installed_version = |workspace: &Path| -> String {
        let manifest = workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json");
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(manifest).expect("read the dep manifest"))
                .expect("parse the dep manifest");
        value["version"].as_str().expect("version string").to_string()
    };

    let pin = |version: &str| {
        let package_json = serde_json::json!({
            "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": version },
        });
        fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    };

    pin("100.0.0");
    pacquet.with_arg("install").assert().success();
    assert_eq!(installed_version(&workspace), "100.0.0");

    let stale_modules = fs::read(workspace.join("node_modules/.modules.yaml")).expect("read state");
    let stale_current = fs::read(workspace.join(CURRENT_LOCKFILE)).expect("read state");

    pin("100.1.0");
    rerun(&workspace).with_arg("install").assert().success();
    assert_eq!(installed_version(&workspace), "100.1.0");

    // Roll both state files back to what the 100.0.0 install wrote while
    // node_modules holds 100.1.0 — the shape an interrupted install or a
    // bad merge leaves behind.
    fs::write(workspace.join("node_modules/.modules.yaml"), &stale_modules).expect("restore state");
    fs::write(workspace.join(CURRENT_LOCKFILE), &stale_current).expect("restore state");

    pin("100.0.0");
    rerun(&workspace).with_arg("install").assert().success();
    assert_eq!(
        installed_version(&workspace),
        "100.0.0",
        "node_modules must be re-derived from the wanted lockfile, not trusted from stale state",
    );

    drop((root, mock_instance));
}

/// TS: `using a global virtual store`
/// (`deps-installer/test/install/globalVirtualStore.ts:21`), the
/// current-lockfile half: the project keeps its own
/// `node_modules/.pnpm/lock.yaml` even though the packages themselves
/// live in the shared store, on both the first install and a frozen
/// reinstall from a wiped `node_modules`.
#[test]
fn a_global_virtual_store_install_still_writes_the_current_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    enable_gvs_in_workspace_yaml(&workspace, "");

    pacquet.with_arg("install").assert().success();
    let expected = package_names(&read_current_lockfile(&workspace));
    assert_eq!(
        expected,
        ["@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0", "@pnpm.e2e/pkg-with-1-dep@100.0.0"],
    );

    fs::remove_dir_all(workspace.join("node_modules")).expect("wipe node_modules");
    rerun(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert_eq!(package_names(&read_current_lockfile(&workspace)), expected);

    drop((root, mock_instance));
}

mod known_failures {
    //! Current-lockfile cases blocked on behavior pacquet has not built
    //! yet. Each stubs the not-yet-built subject under test through
    //! [`pacquet_testing_utils::allow_known_failure`] so the test exits
    //! early rather than masking a real bug.

    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    fn current_lockfile_parse_failure_is_fatal() -> KnownResult<()> {
        Err(KnownFailure::new(
            "pacquet fails the install with `ERR_PNPM_BROKEN_LOCKFILE` when \
             `<virtual_store_dir>/lock.yaml` does not parse \
             (`Install::run` maps the load error through \
             `InstallError::LoadCurrentLockfile`). Upstream treats the \
             current lockfile as a disposable cache: it warns `Ignoring \
             broken lockfile at ...` and continues with an empty one, \
             since every fact in it can be re-derived from \
             `pnpm-lock.yaml` and the filesystem. Making pacquet degrade \
             the same way means demoting that error to a warn plus a \
             `pnpm:` log emission the default reporter renders, which is \
             a behavior change worth its own change.",
        ))
    }

    /// TS: `a broken private lockfile is ignored`
    /// (`deps-installer/test/lockfile.ts:1351`).
    #[test]
    fn a_broken_current_lockfile_is_ignored_with_a_warning() {
        allow_known_failure!(current_lockfile_parse_failure_is_fatal());
    }
}
