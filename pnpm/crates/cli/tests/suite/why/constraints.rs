use std::fs;

use assert_cmd::prelude::*;

use super::{DEP, PKG, pacquet, setup, write_manifest};

#[test]
fn why_json_output_includes_requires() {
    let (_root, workspace, _anchor) = setup();
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "project", "version": "0.0.0", "dependencies": {{ "{PKG}": "100.0.0" }} }}"#,
        ),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output =
        pacquet(&workspace, ["why", "--json", "--prod", DEP]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse why JSON");
    let matched = parsed
        .as_array()
        .expect("array output")
        .iter()
        .find(|result| result["name"] == DEP)
        .expect("the queried package is present");
    let dependents = matched["dependents"].as_array().expect("dependents array");
    let pkg_node = dependents
        .iter()
        .find(|node| node["name"] == PKG)
        .expect("package dependent is present");
    assert_eq!(pkg_node["requires"], "^100.0.0");
    let importer_node = pkg_node["dependents"][0].as_object().expect("importer dependent object");
    assert_eq!(importer_node.get("requires").and_then(serde_json::Value::as_str), Some("100.0.0"));
}

#[test]
fn why_preserves_selected_dependency_constraints() {
    let (_root, workspace, _anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let mut lockfile = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace).unwrap().unwrap();
    let importer = lockfile.importers.get_mut(".").unwrap();
    let mut dev_dependencies = importer.dependencies.clone().unwrap();
    for spec in dev_dependencies.values_mut() {
        spec.specifier = "^100.0.0".to_string();
    }
    importer.dev_dependencies = Some(dev_dependencies);
    lockfile
        .save_to_path(&workspace.join("pnpm-lock.yaml"), pnpm_lockfile::SymlinkedLockfile::Refuse)
        .unwrap();
    lockfile
        .save_current_to_install_state_dir(&workspace.join("node_modules/.pnpm"))
        .unwrap();

    for (flag, field, requires) in
        [("--prod", "dependencies", "100.0.0"), ("--dev", "devDependencies", "^100.0.0")]
    {
        let output = pacquet(&workspace, ["why", "--json", flag, PKG]).output().unwrap();
        assert!(output.status.success(), "{flag}: {output:?}");
        let tree: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let dependents = tree[0]["dependents"].as_array().unwrap();
        assert_eq!(dependents.len(), 1, "{flag}: {tree}");
        assert_eq!(dependents[0]["requires"], requires, "{flag}: {tree}");
        assert_eq!(dependents[0]["depField"], field, "{flag}: {tree}");
    }
}
