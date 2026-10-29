//! The lockfile checks read a directory package's manifest with the hooks
//! the resolver applies to it: a `file:` dependency or an injected
//! `workspace:` dependency that no workspace project describes, at any depth
//! and in either lockfile layout.

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

/// A `file:` dependency on another package's `publishConfig.directory` reads
/// the directory's own `package.json`. The publishing package's manifest is
/// what a workspace spec would read, so the check does not hook it.
#[test]
fn file_dependency_on_a_publish_directory_hooks_only_its_own_manifest() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest("root", ManifestDeps::default());
    write_package(
        &fixture.workspace.join("ext"),
        &json!({ "name": "ext-src", "version": "1.0.0", "publishConfig": { "directory": "dist" } }),
    );
    write_package(
        &fixture.workspace.join("ext/dist"),
        &json!({ "name": "ext", "version": "1.0.0" }),
    );
    write_package(
        &fixture.workspace.join("packages/app"),
        &json!({ "name": "app", "version": "1.0.0", "dependencies": { "ext": "file:../../ext/dist" } }),
    );
    fs::write(
        fixture.workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { readPackage(pkg) {\n\
           if (pkg.name === 'ext-src') throw new Error('ext-src is not a dependency');\n\
           return pkg;\n\
         } } };\n",
    )
    .expect("write .pnpmfile.cjs");

    install(&fixture, &[]);
    install(&fixture, &["--frozen-lockfile"]);
}

/// With its own lockfile, a project reaching one publish directory through
/// both a workspace spec and a `file:` spec checks a nested workspace range
/// on it against the publishing project's version, not the directory's own.
#[test]
fn publish_directory_reached_both_ways_keeps_the_workspace_version() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(
        "sharedWorkspaceLockfile: false\ninjectWorkspacePackages: true\n",
    );
    fixture.write_root_manifest("root", ManifestDeps::default());
    write_package(
        &fixture.workspace.join("packages/lib"),
        &json!({ "name": "lib", "version": "1.0.0", "publishConfig": { "directory": "dist" } }),
    );
    write_package(
        &fixture.workspace.join("packages/lib/dist"),
        &json!({ "name": "lib", "version": "2.0.0" }),
    );
    write_package(
        &fixture.workspace.join("ext"),
        &json!({ "name": "ext", "version": "1.0.0", "dependencies": { "lib": "workspace:^1.0.0" } }),
    );
    write_package(
        &fixture.workspace.join("packages/app"),
        &json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "lib": "workspace:^1.0.0",
                "lib-dist": "file:../lib/dist",
                "ext": "file:../../ext",
            },
        }),
    );

    install(&fixture, &[]);
    install(&fixture, &["--frozen-lockfile"]);
}

/// A `workspace:` dependency on a directory outside the workspace patterns is
/// a directory package no project describes, like a `file:` dependency, so
/// the check hooks its manifest in either lockfile layout
/// ([pnpm/pnpm#17473](https://github.com/pnpm/pnpm/issues/17473)). `app` is a
/// project beside the root so that a dedicated lockfile is its own file.
#[test]
fn package_extensions_on_an_injected_directory_outside_the_patterns_pass_frozen_install() {
    // Each extension and the lockfile lines that prove the resolver applied it;
    // the dependency's recorded path is relative to the lockfile, so only its
    // start is compared.
    let extensions = [
        (
            "packageExtensions:\n  peer@1.0.0:\n    peerDependencies:\n      host: 1.0.0\n",
            "    peerDependencies:\n      host: 1.0.0\n",
        ),
        (
            "packageExtensions:\n  peer@1.0.0:\n    dependencies:\n      host: workspace:../host\n",
            "    dependencies:\n      host: file:",
        ),
    ];
    for shared_lockfile in [true, false] {
        for (extension, applied) in extensions {
            let fixture = WorkspaceFixture::new();
            fixture.append_workspace_yaml(&format!(
                "injectWorkspacePackages: true\npreferWorkspacePackages: true\n\
                 sharedWorkspaceLockfile: {shared_lockfile}\n{extension}",
            ));
            let outside = fixture.workspace.parent().expect("the workspace has a parent");
            write_package(&outside.join("peer"), &json!({ "name": "peer", "version": "1.0.0" }));
            write_package(&outside.join("host"), &json!({ "name": "host", "version": "1.0.0" }));
            fixture.write_root_manifest("root", ManifestDeps::default());
            let app = fixture.workspace.join("packages/app");
            write_package(
                &app,
                &json!({
                    "name": "app",
                    "version": "1.0.0",
                    "dependencies": {
                        "peer": "workspace:../../../peer",
                        "host": "workspace:../../../host",
                    },
                }),
            );

            install(&fixture, &[]);
            let lockfile_dir = if shared_lockfile { &fixture.workspace } else { &app };
            let lockfile =
                fs::read_to_string(lockfile_dir.join("pnpm-lock.yaml")).expect("read the lockfile");
            assert!(
                lockfile.contains(applied),
                "shared_lockfile: {shared_lockfile}, the extension reached peer's entry:\n{lockfile}",
            );
            install(&fixture, &["--frozen-lockfile"]);
        }
    }
}
