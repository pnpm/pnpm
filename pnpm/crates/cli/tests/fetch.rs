use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// Direct dependency from each group, so a test can assert which groups
/// the `--prod` / `--dev` flags fetch. Each is a leaf package (no
/// transitive deps), so it only ever reaches `node_modules` as that
/// group's direct dependency — never hoisted in on another's behalf.
const PROD_DEP: &str = "@pnpm.e2e/foo";
const DEV_DEP: &str = "@pnpm.e2e/bar";
const OPTIONAL_DEP: &str = "@pnpm.e2e/qar";

/// Top-level `node_modules` symlink fetch creates for a direct
/// dependency. `--prod` / `--dev` filter which groups are symlinked
/// here (the virtual store under `.pnpm` mirrors the whole lockfile
/// regardless), so this is what proves the dependency-group filter.
fn direct_dep_link(workspace: &Path, name: &str) -> std::path::PathBuf {
    workspace.join("node_modules").join(name)
}

/// Write a manifest pinning one dependency per group, then materialize a
/// lockfile with `install --lockfile-only` (which resolves every group
/// without populating the store).
fn write_manifest_and_lockfile(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { PROD_DEP: "100.0.0" },
            "devDependencies": { DEV_DEP: "100.0.0" },
            "optionalDependencies": { OPTIONAL_DEP: "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet_at(workspace).with_args(["install", "--lockfile-only"]).assert().success();
    assert!(workspace.join("pnpm-lock.yaml").exists(), "lockfile must exist after --lockfile-only");
}

#[test]
fn fetch_requires_existing_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { PROD_DEP: "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = pacquet.with_arg("fetch").output().expect("spawn pacquet fetch");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "fetch without lockfile must fail (stderr: {stderr})");
    assert!(
        stderr.contains("pnpm-lock.yaml"),
        "fetch must fail specifically because the lockfile is missing (stderr: {stderr})",
    );

    drop((root, mock_instance));
}

#[test]
fn fetch_populates_every_group_by_default() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest_and_lockfile(&workspace);

    pacquet_at(&workspace).with_arg("fetch").assert().success();

    assert!(store_dir.join("v11").exists(), "fetch must populate the store");
    assert!(direct_dep_link(&workspace, PROD_DEP).exists(), "production dep must be fetched");
    assert!(direct_dep_link(&workspace, DEV_DEP).exists(), "dev dep must be fetched");
    assert!(direct_dep_link(&workspace, OPTIONAL_DEP).exists(), "optional dep must be fetched");

    drop((root, mock_instance, store_dir));
}

#[test]
fn fetch_prod_keeps_optional_drops_dev() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest_and_lockfile(&workspace);

    pacquet_at(&workspace).with_args(["fetch", "--prod"]).assert().success();

    assert!(
        direct_dep_link(&workspace, PROD_DEP).exists(),
        "`fetch --prod` must fetch production deps",
    );
    assert!(
        direct_dep_link(&workspace, OPTIONAL_DEP).exists(),
        "`fetch --prod` must still fetch optional deps (they follow production)",
    );
    assert!(
        !direct_dep_link(&workspace, DEV_DEP).exists(),
        "`fetch --prod` must not fetch dev deps",
    );

    drop((root, mock_instance));
}

#[test]
fn fetch_dev_drops_prod_and_optional() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest_and_lockfile(&workspace);

    pacquet_at(&workspace).with_args(["fetch", "--dev"]).assert().success();

    assert!(direct_dep_link(&workspace, DEV_DEP).exists(), "`fetch --dev` must fetch dev deps");
    assert!(
        !direct_dep_link(&workspace, PROD_DEP).exists(),
        "`fetch --dev` must not fetch production deps",
    );
    assert!(
        !direct_dep_link(&workspace, OPTIONAL_DEP).exists(),
        "`fetch --dev` must not fetch optional deps (they follow production)",
    );

    drop((root, mock_instance));
}
