use crate::_utils::{
    has_link,
    importer_has_group_dependency,
    read_lockfile,
};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{
    AddMockedRegistry,
    CommandTempCwd,
};
use std::fs;

const PROD: &str = "@pnpm.e2e/pkg-with-1-dep";
const FILTERED: &str = "@pnpm.e2e/hello-world-js-bin";

#[test]
fn prune_writes_lockfile() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

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

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    pacquet
        .with_arg("prune")
        .assert()
        .success();

    assert!(lockfile_path.exists(), "prune must create pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep"),
        "lockfile must record the dependency:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn prune_from_workspace_member_writes_the_workspace_lockfile() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(workspace.join("package.json"), r#"{ "name": "workspace-root" }"#)
        .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    let member = workspace.join("packages/app");
    fs::create_dir_all(&member).expect("create workspace member");
    fs::write(
        member.join("package.json"),
        r#"{ "name": "app", "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" } }"#,
    )
    .expect("write member package.json");

    pacquet
        .with_current_dir(&member)
        .with_arg("prune")
        .assert()
        .success();

    assert!(workspace.join("pnpm-lock.yaml").is_file());
    assert!(!member.join("pnpm-lock.yaml").exists());

    drop((root, mock_instance));
}

/// `pnpm prune` is `pnpm install` plus direct-dependency pruning, so a
/// group filter reaches `node_modules` and leaves `pnpm-lock.yaml`
/// describing the whole manifest (pnpm/pnpm#14912).
///
/// The manifest declares [`PROD`] in `dependencies` and [`FILTERED`] in
/// `filtered_group`; `unlinked` names the one `filter` drops.
fn assert_prune_filter_reaches_node_modules_only(
    filter: &str,
    filtered_group: &str,
    unlinked: &str,
) {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { PROD: "100.0.0" },
            filtered_group: { FILTERED: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_args(["prune", filter])
        .assert()
        .success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    dbg!(&lockfile);
    let root_importer = pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY;
    assert!(
        importer_has_group_dependency(&lockfile, root_importer, "dependencies", PROD),
        "`prune {filter}` dropped dependencies from the lockfile",
    );
    assert!(
        importer_has_group_dependency(&lockfile, root_importer, filtered_group, FILTERED),
        "`prune {filter}` dropped {filtered_group} from the lockfile",
    );
    let linked = if unlinked == PROD { FILTERED } else { PROD };
    assert!(has_link(&workspace, linked), "`prune {filter}` must keep the installed group linked");
    assert!(!has_link(&workspace, unlinked), "`prune {filter}` must unlink the group it excludes");

    drop((root, mock_instance));
}

#[test]
fn prune_with_prod_only_unlinks_dev_deps() {
    assert_prune_filter_reaches_node_modules_only("--prod", "devDependencies", FILTERED);
}

#[test]
fn prune_with_dev_only_unlinks_prod_deps() {
    assert_prune_filter_reaches_node_modules_only("--dev", "devDependencies", PROD);
}

#[test]
fn prune_with_no_optional_unlinks_optional_deps() {
    assert_prune_filter_reaches_node_modules_only(
        "--no-optional",
        "optionalDependencies",
        FILTERED,
    );
}
