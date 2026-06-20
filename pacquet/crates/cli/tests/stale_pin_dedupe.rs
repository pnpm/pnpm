//! Refreshing a stale transitive pin to a higher direct-dependency
//! version during resolution, so an incremental install converges to what
//! a fresh install would produce. Mirrors the pnpm fix in
//! `installing/deps-resolver/src/resolveDependencies.ts`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

fn write_manifest(path: &Path, dep_of_version: &str) {
    fs::write(
        path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/dep-of-pkg-with-1-dep": dep_of_version,
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            }
        })
        .to_string(),
    )
    .expect("write package.json");
}

#[test]
fn refreshes_stale_transitive_pin_to_higher_direct_dep_version() {
    // Keep `root` (the TempDir) and the mock registry alive for the whole
    // test — dropping them deletes the workspace / stops the registry.
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let lockfile_path = workspace.join("pnpm-lock.yaml");

    // First install pins the direct dep at 100.0.0, so the transitive
    // `^100.0.0` edge prefers it: only 100.0.0 is recorded.
    write_manifest(&manifest_path, "100.0.0");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0"),
        "the first install records the pinned 100.0.0:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "the first install must not yet contain 100.1.0:\n{lockfile}",
    );

    // Bump the direct dependency to 100.1.0. It still satisfies the
    // transitive `^100.0.0`, so both edges must land on 100.1.0 and the
    // stale 100.0.0 must be pruned.
    write_manifest(&manifest_path, "100.1.0");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "the second install resolves the bumped 100.1.0:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0"),
        "the stale 100.0.0 transitive pin must be refreshed to 100.1.0, not kept:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn refreshes_stale_transitive_pin_for_caret_range_direct_dep() {
    // Same convergence, but the bumped direct dep uses a caret range
    // (`^100.1.0`) — the spec `pnpm add` actually writes. The refresh
    // relies on the resolved direct-dep version (100.1.0), not the spec
    // string, so it must still redirect the transitive `^100.0.0` edge.
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let lockfile_path = workspace.join("pnpm-lock.yaml");

    write_manifest(&manifest_path, "100.0.0");
    pacquet_at(&workspace).with_arg("install").assert().success();

    write_manifest(&manifest_path, "^100.1.0");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "the bumped caret-range direct dep resolves 100.1.0:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0"),
        "the stale 100.0.0 transitive pin must be refreshed to 100.1.0, not kept:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn does_not_refresh_an_aliased_transitive_dependency() {
    // pkg-with-1-aliased-dep depends on
    // `dep: npm:@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0`. An `npm:` specifier
    // is not a plain semver range, so the refresh skips the edge and the
    // older version is kept (no misfire on aliases). Matches pnpm.
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let lockfile_path = workspace.join("pnpm-lock.yaml");

    let write = |dep_of_version: &str| {
        fs::write(
            &manifest_path,
            serde_json::json!({
                "dependencies": {
                    "@pnpm.e2e/dep-of-pkg-with-1-dep": dep_of_version,
                    "@pnpm.e2e/pkg-with-1-aliased-dep": "100.0.0",
                }
            })
            .to_string(),
        )
        .expect("write package.json");
    };

    write("100.0.0");
    pacquet_at(&workspace).with_arg("install").assert().success();

    write("100.1.0");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "the bumped direct dep resolves 100.1.0:\n{lockfile}",
    );
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0"),
        "the aliased transitive edge keeps its 100.0.0 pin:\n{lockfile}",
    );

    drop((root, mock_instance));
}
