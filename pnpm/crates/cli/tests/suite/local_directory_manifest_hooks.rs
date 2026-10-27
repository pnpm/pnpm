//! The lockfile checks read a `file:` directory package's manifest with the
//! hooks the resolver applies to it, at any depth and in either lockfile
//! layout.

use crate::_utils::{
    ManifestDeps, WorkspaceFixture, assert_success, ndjson_records, write_manifest_value,
};
use serde_json::{Value, json};
use std::{fs, path::Path};

/// A workspace whose `app` depends on `ext`, a `file:` directory outside the
/// workspace patterns, so no project describes it.
fn file_dependency_workspace(shared_lockfile: bool, ext_manifest: &Value) -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    if !shared_lockfile {
        fixture.append_workspace_yaml("sharedWorkspaceLockfile: false\n");
    }
    fixture.write_root_manifest("root", ManifestDeps::default());
    write_package(&fixture.workspace.join("ext"), ext_manifest);
    write_package(
        &fixture.workspace.join("packages/app"),
        &json!({ "name": "app", "version": "1.0.0", "dependencies": { "ext": "file:../../ext" } }),
    );
    fixture
}

fn write_package(dir: &Path, manifest: &Value) {
    fs::create_dir_all(dir).expect("create package directory");
    write_manifest_value(dir, manifest);
}

fn install(fixture: &WorkspaceFixture, args: &[&str]) -> Vec<Value> {
    let output = fixture.command_at(&fixture.workspace, std::iter::once(&"install").chain(args));
    assert_success(&output);
    ndjson_records(&output)
}

#[test]
fn read_package_hook_on_a_file_dependency_passes_frozen_install() {
    for shared_lockfile in [true, false] {
        let fixture = file_dependency_workspace(
            shared_lockfile,
            &json!({ "name": "ext", "version": "1.0.0", "dependencies": { "gone": "file:./missing" } }),
        );
        write_package(
            &fixture.workspace.join("ext/real"),
            &json!({ "name": "real", "version": "1.0.0" }),
        );
        write_package(
            &fixture.workspace.join("ext/real/nested"),
            &json!({ "name": "nested", "version": "1.0.0", "dependencies": { "gone": "file:./missing" } }),
        );
        fs::write(
            fixture.workspace.join(".pnpmfile.cjs"),
            "module.exports = { hooks: { readPackage(pkg) {\n\
               if (pkg.name === 'ext') pkg.dependencies = { real: 'file:./real' };\n\
               if (pkg.name === 'real') pkg.dependencies = { nested: 'file:./nested' };\n\
               if (pkg.name === 'nested') pkg.dependencies = {};\n\
               return pkg;\n\
             } } };\n",
        )
        .expect("write .pnpmfile.cjs");

        install(&fixture, &[]);
        install(&fixture, &["--frozen-lockfile"]);
    }
}

#[test]
fn ignored_optional_dependency_of_a_file_dependency_does_not_resolve_again() {
    for shared_lockfile in [true, false] {
        let fixture = file_dependency_workspace(
            shared_lockfile,
            &json!({ "name": "ext", "version": "1.0.0", "optionalDependencies": { "opt": "file:./opt" } }),
        );
        write_package(
            &fixture.workspace.join("ext/opt"),
            &json!({ "name": "opt", "version": "1.0.0" }),
        );
        fixture.append_workspace_yaml("ignoredOptionalDependencies:\n  - opt\n");

        install(&fixture, &[]);
        let resolved_again = install(&fixture, &[])
            .iter()
            .any(|record| {
                record["name"] == "pnpm:stage" && record["stage"] == "resolution_started"
            });
        assert!(!resolved_again, "shared_lockfile: {shared_lockfile}");
    }
}
