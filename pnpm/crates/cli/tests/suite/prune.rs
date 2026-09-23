use crate::_utils::{has_link, importer_has_group_dependency, pacquet_in, read_lockfile};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
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
fn prune_with_prod_only_and_no_lockfile_unlinks_dev_deps() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { PROD: "100.0.0" },
            "devDependencies": { FILTERED: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "lockfile: false\n")
        .expect("write pnpm-workspace.yaml");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(!workspace.join("pnpm-lock.yaml").exists(), "install must not write a lockfile");
    assert!(has_link(&workspace, PROD));
    assert!(has_link(&workspace, FILTERED));

    pacquet_in(&workspace)
        .with_args(["prune", "--prod"])
        .assert()
        .success();
    assert!(has_link(&workspace, PROD));
    assert!(!has_link(&workspace, FILTERED));
    assert!(!workspace.join("pnpm-lock.yaml").exists(), "prune must not write a lockfile");

    drop((root, mock_instance));
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

fn append_order_script(stage: &str) -> String {
    format!(r#"node -e "require('fs').appendFileSync('order.txt','{stage}\n')""#)
}

/// `prepare` needs a devDependency, like a `husky` git-hooks setup, so
/// running it after `prune --prod` unlinked that dependency would fail
/// the prune (pnpm/pnpm#4770).
#[test]
fn prune_with_prod_only_does_not_run_prepare_scripts() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let prepare =
        format!(r#"node -e "require('is-negative')" && {}"#, append_order_script("prepare"));
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "prune-prod-skips-prepare",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
            "devDependencies": { "is-negative": "1.0.0" },
            "scripts": {
                "preinstall": append_order_script("preinstall"),
                "install": append_order_script("install"),
                "postinstall": append_order_script("postinstall"),
                "preprepare": append_order_script("preprepare"),
                "prepare": prepare,
                "postprepare": append_order_script("postprepare"),
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let order_path = workspace.join("order.txt");
    let read_stages = || {
        fs::read_to_string(&order_path)
            .expect("read order.txt")
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(
        read_stages(),
        ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare"],
    );
    fs::remove_file(&order_path).expect("remove order.txt");

    pacquet_in(&workspace)
        .with_args(["prune", "--prod"])
        .assert()
        .success();

    assert_eq!(
        read_stages(),
        ["preinstall", "install", "postinstall"],
        "prepare lifecycle scripts must not run during prune --prod",
    );
    assert!(has_link(&workspace, "is-positive"));
    assert!(!has_link(&workspace, "is-negative"));

    drop((root, mock_instance));
}
