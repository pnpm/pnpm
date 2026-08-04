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

fn virtual_dep(workspace: &Path, name: &str) -> std::path::PathBuf {
    let slot = format!("{}@100.0.0", name.replace('/', "+"));
    workspace.join("node_modules/.pnpm").join(slot).join("node_modules").join(name)
}

fn assert_no_importer_links(workspace: &Path) {
    for name in [PROD_DEP, DEV_DEP, OPTIONAL_DEP] {
        assert!(
            !workspace.join("node_modules").join(name).exists(),
            "fetch must not create an importer link for {name}",
        );
    }
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
    assert!(virtual_dep(&workspace, PROD_DEP).exists(), "production dep must be fetched");
    assert!(virtual_dep(&workspace, DEV_DEP).exists(), "dev dep must be fetched");
    assert!(virtual_dep(&workspace, OPTIONAL_DEP).exists(), "optional dep must be fetched");
    assert_no_importer_links(&workspace);
    assert_eq!(
        pacquet_modules_yaml::read_modules_manifest::<pacquet_modules_yaml::Host>(
            &workspace.join("node_modules"),
        )
        .expect("read .modules.yaml")
        .expect("fetch must write .modules.yaml")
        .virtual_store_only,
        Some(true),
    );

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
        virtual_dep(&workspace, PROD_DEP).exists(),
        "`fetch --prod` must fetch production deps",
    );
    assert!(
        virtual_dep(&workspace, OPTIONAL_DEP).exists(),
        "`fetch --prod` must still fetch optional deps (they follow production)",
    );
    assert!(!virtual_dep(&workspace, DEV_DEP).exists(), "`fetch --prod` must not fetch dev deps");
    assert_no_importer_links(&workspace);

    drop((root, mock_instance));
}

#[test]
fn fetch_dev_drops_prod_and_optional() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest_and_lockfile(&workspace);

    pacquet_at(&workspace).with_args(["fetch", "--dev"]).assert().success();

    assert!(virtual_dep(&workspace, DEV_DEP).exists(), "`fetch --dev` must fetch dev deps");
    assert!(
        !virtual_dep(&workspace, PROD_DEP).exists(),
        "`fetch --dev` must not fetch production deps",
    );
    assert!(
        !virtual_dep(&workspace, OPTIONAL_DEP).exists(),
        "`fetch --dev` must not fetch optional deps (they follow production)",
    );
    assert_no_importer_links(&workspace);

    drop((root, mock_instance));
}

#[test]
fn fetch_populates_the_global_virtual_store_without_importer_links() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    write_manifest_and_lockfile(&workspace);
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path)
        .expect("read pnpm-workspace.yaml")
        .replace("enableGlobalVirtualStore: false", "enableGlobalVirtualStore: true");
    fs::write(&yaml_path, yaml).expect("enable the global virtual store");

    pacquet_at(&workspace).with_arg("fetch").assert().success();

    let gvs_root = store_dir.join(pacquet_store_dir::STORE_VERSION).join("links");
    assert!(gvs_root.is_dir(), "fetch must populate the global virtual store");
    assert!(
        gvs_root.join(PROD_DEP).join("100.0.0").is_dir(),
        "the production dependency must have a GVS version directory",
    );
    assert_no_importer_links(&workspace);

    drop((root, mock_instance));
}
