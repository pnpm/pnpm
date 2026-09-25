pub use _utils::*;

use crate::_utils;
use pnpm_testing_utils::fixtures::minimal_tarball;
use pretty_assertions::assert_eq;
use std::{collections::BTreeSet, fs};

#[test]
fn frozen_install_ignores_removed_workspace_projects() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest("root", ManifestDeps::default());
    let active = fixture.project(
        "active",
        "active",
        ManifestDeps { prod: &[("@pnpm.e2e/hello-world-js-bin", "1.0.0")], ..Default::default() },
    );
    let removed = fixture.project(
        "removed",
        "removed",
        ManifestDeps {
            prod: &[("@pnpm.e2e/pkg-with-1-dep", "100.0.0"), ("tiny", "file:tiny.tgz")],
            ..Default::default()
        },
    );
    fs::write(removed.join("tiny.tgz"), minimal_tarball("tiny", "1.0.0")).unwrap();
    fixture.run(["install", "--lockfile-only"]);
    let wanted = fs::read(fixture.workspace.join("pnpm-lock.yaml")).unwrap();
    let workspace_yaml = fixture.workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&workspace_yaml).unwrap();
    fs::write(workspace_yaml, yaml.replace("'packages/*'", "'packages/active'")).unwrap();

    for missing_tarball in [false, true] {
        if missing_tarball {
            fs::remove_dir_all(fixture.workspace.join("node_modules")).unwrap();
            fs::remove_dir_all(active.join("node_modules")).unwrap();
            fs::remove_file(removed.join("tiny.tgz")).unwrap();
        }
        fixture.run(["install", "--frozen-lockfile"]);

        eprintln!("Checking removed workspace project, missing tarball: {missing_tarball}");
        assert!(has_link(&active, "@pnpm.e2e/hello-world-js-bin"));
        assert!(!removed.join("node_modules").exists());
        assert!(!fixture.slot("@pnpm.e2e/pkg-with-1-dep", "100.0.0").exists());
        assert!(!fixture.slot("@pnpm.e2e/dep-of-pkg-with-1-dep", "100.1.0").exists());
        assert_eq!(fs::read(fixture.workspace.join("pnpm-lock.yaml")).unwrap(), wanted);
        assert_eq!(
            importer_ids(&fixture.current()),
            BTreeSet::from([".".to_string(), "packages/active".to_string()]),
        );
    }
}

#[test]
fn fetch_keeps_lockfile_importers_without_project_manifests() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest("root", ManifestDeps::default());
    let project = fixture.project(
        "project",
        "project",
        ManifestDeps { prod: &[("@pnpm.e2e/hello-world-js-bin", "1.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);
    fs::remove_dir_all(project).unwrap();
    fs::remove_file(fixture.workspace.join("package.json")).unwrap();

    fixture.run(["fetch"]);

    eprintln!("Checking that fetch retains projects from the lockfile");
    assert!(fixture.slot("@pnpm.e2e/hello-world-js-bin", "1.0.0").exists());
    assert_eq!(importer_ids(&fixture.current()), importer_ids(&fixture.wanted()));
}
