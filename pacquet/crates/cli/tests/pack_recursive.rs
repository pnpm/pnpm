//! Recursive-pack integration tests. `--filter` must narrow which
//! projects get packed, routed through the same shared selection path
//! (`select_recursive_projects`) that `run -r` / `exec -r` use.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{fs, path::Path};

/// Write a `pnpm-workspace.yaml` listing `names` as packages, plus a
/// `package.json` (name + version) per name under its own subdirectory.
fn write_workspace(workspace: &Path, names: &[&str]) {
    let packages = names.iter().map(|name| format!("  - {name}")).collect::<Vec<_>>();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n{}\n", packages.join("\n")),
    )
    .expect("write pnpm-workspace.yaml");
    for name in names {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(
            dir.join("package.json"),
            json!({ "name": name, "version": "1.0.0" }).to_string(),
        )
        .expect("write package.json");
    }
}

/// `pacquet -r --filter <name> pack` packs only the `--filter`-selected
/// project, leaving the rest unpacked — the same selection `run -r` /
/// `exec -r` apply, since all three share `select_recursive_projects`.
#[test]
fn recursive_pack_filter_packs_only_selected_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2", "project-3"]);
    let out = workspace.join("tarballs");
    fs::create_dir_all(&out).expect("create out dir");

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("project-1")
        .with_arg("pack")
        .with_arg("--pack-destination")
        .with_arg(out.to_str().expect("utf8 out dir"))
        .assert()
        .success();

    assert!(out.join("project-1-1.0.0.tgz").exists(), "the selected project-1 should be packed");
    for name in ["project-2", "project-3"] {
        assert!(
            !out.join(format!("{name}-1.0.0.tgz")).exists(),
            "{name} is not selected by --filter and must not be packed",
        );
    }

    drop(root);
}
