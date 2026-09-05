use crate::_utils::{ManifestDeps, pacquet_in, read_manifest, write_project_manifest};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, fixtures::minimal_tarball, fs::bump_mtime};
use std::fs;

#[test]
fn frozen_replay_installs_local_overrides_at_different_importer_depths() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_manifest(&workspace, "root", ManifestDeps::default());
    write_project_manifest(&workspace.join("linked"), "linked", ManifestDeps::default());
    fs::write(workspace.join("vendored.tgz"), minimal_tarball("vendored", "1.0.0"))
        .expect("write local tarball");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages: ['packages/*', 'packages/nested/*']\n\
         storeDir: ../pacquet-store\n\
         cacheDir: ../pacquet-cache\n\
         enableGlobalVirtualStore: false\n\
         offline: true\n\
         overrides:\n  vendored: file:./vendored.tgz\n  linked: link:./linked\n",
    )
    .expect("write workspace settings");
    for (project, name) in [("packages/a", "a"), ("packages/nested/b", "b")] {
        write_project_manifest(
            &workspace.join(project),
            name,
            ManifestDeps {
                prod: &[("vendored", "^1.0.0")],
                dev: &[("linked", "^1.0.0")],
                ..ManifestDeps::default()
            },
        );
    }

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read(&lockfile_path).expect("read generated lockfile");

    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_eq!(fs::read(&lockfile_path).unwrap(), lockfile);
    for project in ["packages/a", "packages/nested/b"] {
        for name in ["vendored", "linked"] {
            let installed = read_manifest(&workspace.join(project).join("node_modules").join(name));
            assert_eq!(installed["name"], name);
            assert_eq!(installed["version"], "1.0.0");
        }
    }

    drop(root);
}

#[test]
fn exec_content_check_accepts_a_root_relative_link_override() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_manifest(&workspace.join("linked"), "linked", ManifestDeps::default());
    for (project, name, spec) in
        [(".", "root", "link:linked"), ("packages/nested/a", "a", "link:../../../linked")]
    {
        write_project_manifest(
            &workspace.join(project),
            name,
            ManifestDeps { prod: &[("linked", spec)], ..ManifestDeps::default() },
        );
    }
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages: ['packages/nested/*']\n\
         storeDir: ../pacquet-store\n\
         cacheDir: ../pacquet-cache\n\
         enableGlobalVirtualStore: false\n\
         offline: true\n\
         verifyDepsBeforeRun: error\n\
         overrides:\n  linked: link:./linked\n",
    )
    .expect("write workspace settings");

    pacquet.with_arg("install").assert().success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read(&lockfile_path).expect("read generated lockfile");
    bump_mtime(&workspace.join("packages/nested/a/package.json"));

    pacquet_in(&workspace.join("packages/nested/a"))
        .with_args(["exec", "node", "-p", "require('linked/package.json').version"])
        .assert()
        .success()
        .stdout("1.0.0\n");
    assert_eq!(fs::read(&lockfile_path).unwrap(), lockfile);

    drop(root);
}
