use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_store_dir::STORE_VERSION;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
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

    assert!(store_dir.join(STORE_VERSION).exists(), "fetch must populate the store");
    assert!(virtual_dep(&workspace, PROD_DEP).exists(), "production dep must be fetched");
    assert!(virtual_dep(&workspace, DEV_DEP).exists(), "dev dep must be fetched");
    assert!(virtual_dep(&workspace, OPTIONAL_DEP).exists(), "optional dep must be fetched");
    assert_no_importer_links(&workspace);
    assert_eq!(
        pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(
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

    let gvs_root = store_dir.join(STORE_VERSION).join("links");
    assert!(gvs_root.is_dir(), "fetch must populate the global virtual store");
    assert!(
        gvs_root.join(PROD_DEP).join("100.0.0").is_dir(),
        "the production dependency must have a GVS version directory",
    );
    assert_no_importer_links(&workspace);

    drop((root, mock_instance));
}

/// `.pnp.cjs` is how a `PnP` project resolves, which makes it a
/// project-level artifact like the importer links `fetch` already skips.
/// Writing it would leave the project claiming to resolve out of a store
/// that `fetch` populated but never linked into.
///
/// `fetch` takes its linker from configuration rather than a flag, so
/// the linker is set in `pnpm-workspace.yaml`.
#[test]
fn fetch_under_pnp_does_not_write_the_loader() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    // Append rather than overwrite: the harness's own workspace manifest
    // carries `storeDir` / `cacheDir` / `registry`, and losing those
    // makes the store assertion below fail for an unrelated reason.
    let workspace_manifest = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_manifest).expect("read pnpm-workspace.yaml");
    yaml.push_str("nodeLinker: pnp\n");
    fs::write(&workspace_manifest, yaml).expect("write pnpm-workspace.yaml");

    write_manifest_and_lockfile(&workspace);
    pacquet_at(&workspace).with_arg("fetch").assert().success();

    assert!(store_dir.join(STORE_VERSION).exists(), "fetch must still populate the store");
    assert!(
        !workspace.join(".pnp.cjs").exists(),
        "fetch must not write the PnP loader: it never linked the project",
    );

    drop((root, mock_instance, store_dir));
}

/// A dependency's lifecycle script resolves a sibling dependency's bin
/// through the per-slot `node_modules/.bin` the virtual store carries.
/// `fetch` runs those scripts, so it has to write those links even
/// though it materializes nothing importer-facing
/// ([pnpm/pnpm#14174](https://github.com/pnpm/pnpm/issues/14174)).
///
/// `@pnpm.e2e/pre-and-postinstall-scripts-example`'s postinstall opens
/// with `hello-world-js-bin`, the bin of its own dependency, and only
/// then writes its marker file.
#[test]
fn fetch_runs_a_build_script_that_calls_a_sibling_dependency_bin() {
    const BUILT_DEP: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_manifest = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&workspace_manifest).expect("read pnpm-workspace.yaml");
    let yaml = format!("{yaml}allowBuilds:\n  '{BUILT_DEP}': true\n");
    fs::write(&workspace_manifest, yaml).expect("write pnpm-workspace.yaml");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { BUILT_DEP: "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_args(["install", "--lockfile-only"]).assert().success();

    // The Docker "fetcher stage" shape the report used: the lockfile and
    // the workspace manifest, with no project manifest to import from.
    fs::remove_file(workspace.join("package.json")).expect("remove package.json");

    pacquet_at(&workspace).with_arg("fetch").assert().success();

    let pkg_dir = workspace
        .join("node_modules/.pnpm")
        .join(format!("{}@1.0.0", BUILT_DEP.replace('/', "+")))
        .join("node_modules")
        .join(BUILT_DEP);
    assert!(
        pkg_dir.join("node_modules/.bin/hello-world-js-bin").exists(),
        "fetch must link the built package's dependency bins next to it",
    );
    assert!(
        pkg_dir.join("generated-by-postinstall.js").exists(),
        "the postinstall script must have run to completion",
    );

    drop((root, mock_instance));
}
