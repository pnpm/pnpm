//! A local directory dependency is described by whichever manifest file its
//! directory holds, and a frozen install accepts the lockfile written from it.

use crate::_utils::{
    ManifestDeps,
    WorkspaceFixture,
    write_manifest_value,
};
use serde_json::json;
use std::{
    fs,
    path::Path,
};

const MANIFESTS: [(&str, &str); 2] = [
    ("package.yaml", "name: lib\nversion: 1.0.0\ndependencies:\n  is-positive: 1.0.0\n"),
    ("package.json5", "{name: 'lib', version: '1.0.0', dependencies: {'is-positive': '1.0.0'}}"),
];

fn write_lib(dir: &Path, (basename, contents): (&str, &str)) {
    fs::create_dir_all(dir).expect("create the lib directory");
    fs::write(dir.join(basename), contents).expect("write the lib manifest");
    fs::write(dir.join("index.js"), "").expect("write index.js");
}

/// Installs, then installs again from the lockfile alone. `lib` must bring
/// the dependency its manifest declares both times.
fn assert_lib_dependency_is_installed_and_frozen(fixture: &WorkspaceFixture) {
    let lib_dependency = || {
        let lib = fs::canonicalize(fixture.workspace.join("packages/app/node_modules/lib"))
            .expect("resolve the installed lib");
        lib.parent()
            .expect("lib sits in a node_modules directory")
            .join("is-positive")
            .join("package.json")
    };
    fixture.run(["install"]);
    assert!(lib_dependency().exists(), "the install links the dependency lib declares");
    fs::remove_dir_all(fixture.workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_dir_all(fixture.workspace.join("packages/app/node_modules"))
        .expect("remove the project's node_modules");
    fixture.run(["install", "--frozen-lockfile"]);
    assert!(lib_dependency().exists(), "the frozen install links the dependency lib declares");
}

#[test]
fn a_file_directory_is_described_by_its_yaml_or_json5_manifest() {
    for manifest in MANIFESTS {
        let fixture = WorkspaceFixture::new();
        fixture.write_root_manifest("root", ManifestDeps::default());
        write_lib(&fixture.workspace.join("lib"), manifest);
        fixture.project(
            "app",
            "app",
            ManifestDeps { prod: &[("lib", "file:../../lib")], ..Default::default() },
        );
        assert_lib_dependency_is_installed_and_frozen(&fixture);
    }
}

/// A workspace project reached by a path gets the manifest it gets when
/// reached by a `workspace:` range.
#[test]
fn an_injected_project_reached_by_path_is_described_by_its_yaml_or_json5_manifest() {
    for manifest in MANIFESTS {
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml("dedupeInjectedDeps: false\n");
        fixture.write_root_manifest("root", ManifestDeps::default());
        write_lib(&fixture.workspace.join("packages/lib"), manifest);
        let app = fixture.workspace.join("packages/app");
        fs::create_dir_all(&app).expect("create the app directory");
        write_manifest_value(
            &app,
            &json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "lib": "workspace:../lib" },
                "dependenciesMeta": { "lib": { "injected": true } },
            }),
        );
        assert_lib_dependency_is_installed_and_frozen(&fixture);
    }
}
