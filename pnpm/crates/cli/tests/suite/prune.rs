use crate::_utils::{
    append_line_script, has_link, importer_has_group_dependency, pacquet_in, read_lockfile,
    set_minimum_release_age,
};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

const PROD: &str = "@pnpm.e2e/pkg-with-1-dep";
const FILTERED: &str = "@pnpm.e2e/hello-world-js-bin";
const ORDER_FILE: &str = "order.txt";

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

/// `prepare` needs a devDependency, like a `husky` git-hooks setup, so
/// running it after `prune --prod` unlinked that dependency would fail
/// the prune (pnpm/pnpm#4770).
#[test]
fn prune_with_prod_only_does_not_run_prepare_scripts() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let prepare = format!(
        r#"node -e "require('is-negative')" && {}"#,
        append_line_script("prepare", ORDER_FILE),
    );
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "prune-prod-skips-prepare",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
            "devDependencies": { "is-negative": "1.0.0" },
            "scripts": {
                "preinstall": append_line_script("preinstall", ORDER_FILE),
                "install": append_line_script("install", ORDER_FILE),
                "postinstall": append_line_script("postinstall", ORDER_FILE),
                "preprepare": append_line_script("preprepare", ORDER_FILE),
                "prepare": prepare,
                "postprepare": append_line_script("postprepare", ORDER_FILE),
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let order_path = workspace.join(ORDER_FILE);
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
    assert_eq!(
        (has_link(&workspace, "is-positive"), has_link(&workspace, "is-negative")),
        (true, false),
        "prune --prod must keep the prod dependency linked and unlink the dev dependency",
    );

    drop((root, mock_instance));
}

/// A store and lockfile restored without the metadata cache, as in a CI
/// cache restore or a Nix sandbox (pnpm/pnpm#16999). Verifying the lockfile
/// needs the metadata `--offline` cannot fetch, so only `trustLockfile`
/// lets the prune through.
#[test]
fn prune_offline_honors_trust_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { PROD: "100.0.0" },
            "devDependencies": { FILTERED: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    set_minimum_release_age(&workspace, 1);
    pacquet_in(&workspace)
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    fs::remove_dir_all(&cache_dir).expect("drop the metadata cache");

    let output = pacquet_in(&workspace)
        .with_args(["prune", "--prod", "--offline"])
        .output()
        .expect("spawn pacquet prune");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    eprintln!("{stderr}");
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_NO_OFFLINE_META"));

    pacquet_in(&workspace)
        .with_args(["prune", "--prod", "--offline", "--config.trust-lockfile=true"])
        .assert()
        .success();
    assert!(has_link(&workspace, PROD));
    assert!(!has_link(&workspace, FILTERED));

    drop((root, mock_instance));
}

#[test]
fn prune_removes_extraneous_direct_packages_when_lockfile_is_up_to_date() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let extraneous_dir = workspace.join("node_modules").join("extraneous-pkg");
    fs::create_dir_all(&extraneous_dir).expect("create extraneous package dir");
    fs::write(
        extraneous_dir.join("package.json"),
        r#"{"name":"extraneous-pkg","version":"1.0.0"}"#,
    )
    .expect("write extraneous package.json");

    let linked_target = workspace.join("linked-extraneous");
    fs::create_dir_all(&linked_target).expect("create linked-extraneous target");
    fs::write(linked_target.join("cli.js"), "#!/usr/bin/env node\n")
        .expect("write linked-extraneous bin");
    fs::write(
        linked_target.join("package.json"),
        r#"{"name":"linked-extraneous","version":"1.0.0","bin":{"linked-extraneous":"cli.js"}}"#,
    )
    .expect("write linked-extraneous package.json");
    let extraneous_link = workspace.join("node_modules").join("linked-extraneous");
    pnpm_fs::symlink_dir(&linked_target, &extraneous_link).expect("symlink linked-extraneous");
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create .bin dir");
    let extraneous_bin =
        bin_dir.join(if cfg!(windows) { "linked-extraneous.cmd" } else { "linked-extraneous" });
    fs::write(&extraneous_bin, "shim").expect("write extraneous bin shim");

    let extraneous_scoped_dir = workspace
        .join("node_modules")
        .join("@extraneous")
        .join("scoped-pkg");
    fs::create_dir_all(&extraneous_scoped_dir).expect("create extraneous scoped package dir");
    fs::write(
        extraneous_scoped_dir.join("package.json"),
        r#"{"name":"@extraneous/scoped-pkg","version":"1.0.0"}"#,
    )
    .expect("write extraneous scoped package.json");

    let non_package_dir = workspace.join("node_modules").join("_tmp");
    fs::create_dir_all(&non_package_dir).expect("create _tmp dir");

    pacquet_in(&workspace)
        .with_arg("prune")
        .assert()
        .success();

    assert!(has_link(&workspace, "is-positive"));
    assert!(!extraneous_dir.exists(), "extraneous direct package must be removed");
    assert!(
        fs::symlink_metadata(&extraneous_link).is_err(),
        "extraneous direct package symlink must be removed",
    );
    assert!(!extraneous_bin.exists(), "extraneous direct package bin shim must be removed");
    assert!(linked_target.exists(), "symlink target must not be deleted");
    assert!(
        !workspace
            .join("node_modules")
            .join("@extraneous")
            .exists(),
        "emptied extraneous scope dir must be removed",
    );
    assert!(non_package_dir.exists(), "non-package _tmp dir must not abort prune");

    // Also verify pruning extraneous direct packages when the project has zero dependencies.
    fs::write(workspace.join("package.json"), r#"{"name":"empty-project","version":"1.0.0"}"#)
        .expect("write empty package.json");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    fs::create_dir_all(&extraneous_dir).expect("recreate extraneous package dir");
    fs::write(
        extraneous_dir.join("package.json"),
        r#"{"name":"extraneous-pkg","version":"1.0.0"}"#,
    )
    .expect("write extraneous package.json");

    pacquet_in(&workspace)
        .with_arg("prune")
        .assert()
        .success();

    assert!(
        !extraneous_dir.exists(),
        "extraneous direct package must be removed even when project has zero dependencies",
    );

    // An in-workspace `node_modules -> src` symlink must never redirect prune into `src`.
    let src_subdir = workspace.join("src").join("components");
    fs::create_dir_all(&src_subdir).expect("create src/components");
    let modules_dir = workspace.join("node_modules");
    if modules_dir.exists() {
        fs::remove_dir_all(&modules_dir).expect("remove node_modules");
    }
    pnpm_fs::symlink_dir(&workspace.join("src"), &modules_dir)
        .expect("symlink node_modules -> src");
    let _ = pacquet_in(&workspace).with_arg("prune").output();
    assert!(
        src_subdir.exists(),
        "in-workspace node_modules symlink must not delete src/components",
    );

    drop((root, mock_instance));
}

#[test]
fn prune_respects_workspace_filter() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "workspace-root",
            "private": true,
            "devDependencies": { FILTERED: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");

    let pkg_a = workspace.join("packages/pkg-a");
    let pkg_b = workspace.join("packages/pkg-b");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::create_dir_all(&pkg_b).expect("create pkg-b");

    fs::write(
        pkg_a.join("package.json"),
        serde_json::json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0", "pkg-b": "workspace:*" },
            "devDependencies": { "is-negative": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write pkg-a package.json");
    fs::write(
        pkg_b.join("package.json"),
        serde_json::json!({
            "name": "pkg-b",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
            "devDependencies": { "is-negative": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write pkg-b package.json");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let extraneous_a = pkg_a.join("node_modules").join("extraneous-a");
    let extraneous_b = pkg_b.join("node_modules").join("extraneous-b");
    fs::create_dir_all(&extraneous_a).expect("create extraneous-a");
    fs::create_dir_all(&extraneous_b).expect("create extraneous-b");

    assert!(has_link(&workspace, FILTERED));
    assert!(has_link(&pkg_a, "is-positive"));
    assert!(has_link(&pkg_a, "is-negative"));
    assert!(has_link(&pkg_b, "is-positive"));
    assert!(has_link(&pkg_b, "is-negative"));

    pacquet_in(&workspace)
        .with_args(["--filter", "pkg-a", "prune", "--prod"])
        .assert()
        .success();

    assert!(has_link(&pkg_a, "is-positive"));
    assert!(!has_link(&pkg_a, "is-negative"), "selected package devDependencies must be pruned");
    assert!(!extraneous_a.exists(), "selected package extraneous dep must be pruned");
    assert!(has_link(&pkg_b, "is-positive"));
    assert!(
        has_link(&pkg_b, "is-negative"),
        "unselected sibling package devDependencies must remain linked",
    );
    assert!(extraneous_b.exists(), "unselected sibling extraneous dep must remain untouched");
    assert!(
        has_link(&workspace, FILTERED),
        "unselected workspace root devDependencies must remain linked",
    );

    drop((root, mock_instance));
}

#[test]
fn prune_supports_dedicated_workspace_lockfiles() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), r#"{"name":"workspace-root","private":true}"#)
        .expect("write root package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write pnpm-workspace.yaml");

    let pkg_a = workspace.join("packages/pkg-a");
    let pkg_b = workspace.join("packages/pkg-b");
    fs::create_dir_all(&pkg_a).expect("create pkg-a");
    fs::create_dir_all(&pkg_b).expect("create pkg-b");

    for (name, dir) in [("pkg-a", &pkg_a), ("pkg-b", &pkg_b)] {
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { "is-positive": "1.0.0" },
                "devDependencies": { "is-negative": "1.0.0" },
            })
            .to_string(),
        )
        .expect("write member package.json");
    }

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert!(has_link(&pkg_a, "is-negative"));
    assert!(has_link(&pkg_b, "is-negative"));

    // Filtered prune with sharedWorkspaceLockfile: false prunes only pkg-a.
    pacquet_in(&workspace)
        .with_args(["--filter", "pkg-a", "prune", "--prod"])
        .assert()
        .success();

    assert!(has_link(&pkg_a, "is-positive"));
    assert!(!has_link(&pkg_a, "is-negative"));
    assert!(has_link(&pkg_b, "is-positive"));
    assert!(has_link(&pkg_b, "is-negative"));

    // Unfiltered prune at the workspace root prunes all workspace projects.
    pacquet_in(&workspace)
        .with_args(["prune", "--prod"])
        .assert()
        .success();

    assert!(has_link(&pkg_b, "is-positive"));
    assert!(!has_link(&pkg_b, "is-negative"));

    // Per-project `packageConfigs.modulesDir` traversing outside the workspace via `..` must be refused.
    let external_modules = root.path().join("external-modules");
    let external_sentinel = external_modules.join(".pnpm").join("sentinel@1.0.0");
    fs::create_dir_all(&external_sentinel).expect("create external sentinel");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\npackageConfigs:\n  pkg-a:\n    modulesDir: ../../../external-modules\n",
    )
    .expect("write pnpm-workspace.yaml with relative external modulesDir");

    let output = pacquet_in(&workspace)
        .with_arg("prune")
        .output()
        .expect("run recursive prune with external modulesDir");
    assert!(!output.status.success(), "prune must reject external packageConfigs.modulesDir");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("refusing prune: modules_dir"),
        "expected containment error in stderr, got: {stderr}",
    );
    assert!(external_sentinel.exists(), "external sentinel must remain untouched");

    drop((root, mock_instance));
}
