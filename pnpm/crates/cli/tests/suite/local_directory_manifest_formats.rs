//! A local directory dependency is described by whichever manifest file its
//! directory holds, and a frozen install accepts the lockfile written from it.

use crate::_utils::{
    ManifestDeps,
    WorkspaceFixture,
    assert_bin_linked,
    write_manifest_value,
    write_marker_script,
};
use serde_json::{
    Value,
    json,
};
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

fn workspace_linking_lib(manifest: (&str, &str), workspace_yaml: &str) -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(workspace_yaml);
    fixture.write_root_manifest("root", ManifestDeps::default());
    write_lib(&fixture.workspace.join("packages/lib"), manifest);
    fixture.project(
        "app",
        "app",
        ManifestDeps { prod: &[("lib", "workspace:^1.0.0")], ..Default::default() },
    );
    fixture
}

/// A filtered install checks the lockfile holding only the selected projects'
/// manifests, so it reads `lib` from disk.
#[test]
fn a_filtered_frozen_install_accepts_a_yaml_or_json5_linked_project() {
    for manifest in MANIFESTS {
        let fixture = workspace_linking_lib(manifest, "");
        fixture.run(["install"]);
        fixture.run(["install", "--frozen-lockfile", "--filter", "app"]);
    }
}

#[test]
fn a_dedicated_lockfile_frozen_install_accepts_a_yaml_or_json5_linked_project() {
    for manifest in MANIFESTS {
        let fixture = workspace_linking_lib(manifest, "sharedWorkspaceLockfile: false\n");
        fixture.run(["install"]);
        fixture.run(["install", "--frozen-lockfile"]);
    }
}

/// JSON is valid JSON5 and valid YAML, so one serialization serves every
/// basename.
fn write_tool(dir: &Path, basename: &str, manifest: &Value) {
    fs::create_dir_all(dir.join("dist")).expect("create the tool's dist directory");
    fs::create_dir_all(dir.join("src")).expect("create the tool's src directory");
    fs::write(dir.join(basename), manifest.to_string()).expect("write the tool manifest");
    fs::write(dir.join("dist/cli.js"), "#!/usr/bin/env node\n").expect("write the bin");
    fs::write(dir.join("src/internal.js"), "").expect("write a file outside `files`");
}

#[test]
fn a_copied_directory_follows_the_files_bin_and_scripts_of_its_yaml_or_json5_manifest() {
    for basename in ["package.yaml", "package.json5"] {
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml("allowBuilds:\n  'tool@file:tool': true\n");
        fixture.write_root_manifest("root", ManifestDeps::default());
        write_tool(
            &fixture.workspace.join("tool"),
            basename,
            &json!({
                "name": "tool",
                "version": "1.0.0",
                "files": ["dist"],
                "bin": { "tool-cli": "dist/cli.js" },
                "scripts": { "postinstall": write_marker_script("built.txt") },
            }),
        );
        fixture.project(
            "app",
            "app",
            ManifestDeps { prod: &[("tool", "file:../../tool")], ..Default::default() },
        );

        fixture.run(["install"]);

        let modules = fixture.workspace.join("packages/app/node_modules");
        assert_bin_linked(&modules.join(".bin").join("tool-cli"));
        assert!(modules.join("tool/dist/cli.js").exists(), "{basename}: `files` selects dist");
        assert!(!modules.join("tool/src").exists(), "{basename}: `files` leaves src out");
        assert!(modules.join("tool/built.txt").exists(), "{basename}: postinstall ran");
    }
}

#[test]
fn a_workspace_project_bin_is_linked_from_its_yaml_or_json5_manifest() {
    for basename in ["package.yaml", "package.json5"] {
        for injected in [false, true] {
            let fixture = WorkspaceFixture::new();
            fixture.write_root_manifest("root", ManifestDeps::default());
            write_tool(
                &fixture.workspace.join("packages/tool"),
                basename,
                &json!({
                    "name": "tool",
                    "version": "1.0.0",
                    "bin": { "tool-cli": "dist/cli.js" },
                }),
            );
            let app = fixture.workspace.join("packages/app");
            fs::create_dir_all(&app).expect("create the app directory");
            write_manifest_value(
                &app,
                &json!({
                    "name": "app",
                    "version": "1.0.0",
                    "dependencies": { "tool": "workspace:*" },
                    "dependenciesMeta": { "tool": { "injected": injected } },
                }),
            );

            fixture.run(["install"]);

            assert_bin_linked(
                &app.join("node_modules")
                    .join(".bin")
                    .join("tool-cli"),
            );
        }
    }
}

#[test]
fn the_loaded_linker_links_a_workspace_project_bin_from_its_yaml_or_json5_manifest() {
    for basename in ["package.yaml", "package.json5"] {
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml("nodeLinker: loaded\n");
        fixture.write_root_manifest("root", ManifestDeps::default());
        write_tool(
            &fixture.workspace.join("packages/tool"),
            basename,
            &json!({
                "name": "tool",
                "version": "1.0.0",
                "bin": { "tool-cli": "dist/cli.js" },
            }),
        );
        let app = fixture.project(
            "app",
            "app",
            ManifestDeps { prod: &[("tool", "workspace:*")], ..Default::default() },
        );

        fixture.run(["install"]);

        assert_bin_linked(
            &app.join("node_modules")
                .join(".bin")
                .join("tool-cli"),
        );
    }
}

#[test]
fn the_loaded_linker_builds_a_copied_directory_with_a_yaml_or_json5_manifest() {
    for basename in ["package.yaml", "package.json5"] {
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml(
            "nodeLinker: loaded\nallowBuilds:\n  'tool@file:tool': true\n",
        );
        fixture.write_root_manifest("root", ManifestDeps::default());
        write_tool(
            &fixture.workspace.join("tool"),
            basename,
            &json!({
                "name": "tool",
                "version": "1.0.0",
                "bin": { "tool-cli": "dist/cli.js" },
                "scripts": { "postinstall": write_marker_script("built.txt") },
            }),
        );
        let app = fixture.project(
            "app",
            "app",
            ManifestDeps { prod: &[("tool", "file:../../tool")], ..Default::default() },
        );

        fixture.run(["install"]);

        assert_bin_linked(
            &app.join("node_modules")
                .join(".bin")
                .join("tool-cli"),
        );
    }
}
