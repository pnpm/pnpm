use super::{
    PrivateWorkspaceDependencyError,
    private_workspace_dependencies,
    reject_private_workspace_dependencies,
};
use crate::{
    CreateExportableManifestOptions,
    WorkspacePackageManifest,
    create_exportable_manifest,
};
use pnpm_catalogs_types::{
    Catalog,
    Catalogs,
};
use serde_json::{
    Value,
    json,
};
use std::{
    collections::HashMap,
    fs,
};
use tempfile::TempDir;

/// A project at `<temp>/app` with `secret` installed under its
/// `node_modules`, and the same `secret` as a sibling at `<temp>/secret`.
fn fixture(secret_private: bool) -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    let secret = json!({ "name": "secret", "version": "1.0.0", "private": secret_private });
    for dir in [temp.path().join("app/node_modules/secret"), temp.path().join("secret")] {
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), secret.to_string()).unwrap();
    }
    temp
}

/// Find the private dependencies, build the published manifest, let
/// `before_packing` change it the way a `beforePacking` hook would, and
/// reject what is left.
fn export_with(
    temp: &TempDir,
    manifest: &Value,
    workspace_packages: Option<&HashMap<String, WorkspacePackageManifest>>,
    before_packing: impl FnOnce(&mut Value),
) -> Result<Value, PrivateWorkspaceDependencyError> {
    let catalogs = Catalogs::from([(
        "default".to_string(),
        Catalog::from([("secret".to_string(), "workspace:*".to_string())]),
    )]);
    let dir = temp.path().join("app");
    let opts = CreateExportableManifestOptions {
        catalogs: &catalogs,
        workspace_dir: None,
        modules_dir: None,
        skip_manifest_obfuscation: false,
        embed_readme: false,
        workspace_packages,
        bumped_workspace_packages: None,
    };
    let private = private_workspace_dependencies(&dir, manifest, &opts).expect("find private");
    let mut published = create_exportable_manifest(&dir, manifest, &opts).expect("exportable");
    before_packing(&mut published);
    reject_private_workspace_dependencies(&published, &private, &[])?;
    Ok(published)
}

fn export(
    temp: &TempDir,
    manifest: &Value,
    workspace_packages: Option<&HashMap<String, WorkspacePackageManifest>>,
) -> Result<Value, PrivateWorkspaceDependencyError> {
    export_with(temp, manifest, workspace_packages, |_| {})
}

fn app(field: &str, spec: &str) -> Value {
    json!({ "name": "app", "version": "1.0.0", field: { "secret": spec } })
}

fn assert_rejected(result: Result<Value, PrivateWorkspaceDependencyError>) {
    let error = result.expect_err("the private dependency is rejected");
    assert_eq!((error.package_name.as_str(), error.dep_name.as_str()), ("app", "secret"));
}

#[test]
fn rejects_a_private_workspace_package_in_dependencies() {
    let temp = fixture(true);
    for spec in
        ["workspace:*", "workspace:^1.0.0", "workspace:secret@*", "workspace:../secret", "catalog:"]
    {
        assert_rejected(export(&temp, &app("dependencies", spec), None));
    }
}

#[test]
fn falls_back_to_the_workspace_packages_when_not_installed() {
    let temp = fixture(true);
    fs::remove_dir_all(temp.path().join("app/node_modules")).unwrap();
    let packages = HashMap::from([(
        "secret".to_string(),
        WorkspacePackageManifest {
            name: "secret".to_string(),
            version: "1.0.0".to_string(),
            private: true,
        },
    )]);
    assert_rejected(export(&temp, &app("dependencies", "workspace:*"), Some(&packages)));
}

#[test]
fn allows_what_does_not_need_the_private_package_from_the_registry() {
    let temp = fixture(true);
    let mut private = app("dependencies", "workspace:*");
    private["private"] = json!(true);
    for manifest in [
        app("devDependencies", "workspace:*"),
        app("optionalDependencies", "workspace:*"),
        app("peerDependencies", "workspace:*"),
        private,
    ] {
        export(&temp, &manifest, None).expect("manifest is exportable");
    }
}

#[test]
fn allows_a_publishable_workspace_package() {
    let temp = fixture(false);
    let published = export(&temp, &app("dependencies", "workspace:*"), None).expect("exportable");
    assert_eq!(published["dependencies"]["secret"], "1.0.0");
}

#[test]
fn ignores_a_registry_dependency_installed_from_elsewhere() {
    let temp = fixture(true);
    export(&temp, &app("dependencies", "^1.0.0"), None).expect("manifest is exportable");
}

#[test]
fn allows_a_dependency_a_before_packing_hook_removes_or_replaces() {
    let temp = fixture(true);
    let manifest = app("dependencies", "workspace:*");
    export_with(&temp, &manifest, None, |published| {
        published["dependencies"]
            .as_object_mut()
            .unwrap()
            .remove("secret");
    })
    .expect("the hook removed the dependency");
    export_with(&temp, &manifest, None, |published| {
        published["dependencies"]["secret"] = json!("npm:public-secret@^1.0.0");
    })
    .expect("the hook pointed the dependency at a public package");
    export_with(&temp, &manifest, None, |published| {
        published["dependencies"]["secret"] = json!(r".\vendor\public-secret.tgz");
    })
    .expect("the hook pointed the dependency at a local tarball");
    for remote in ["https://example.com/public-secret.tgz", "github:owner/public-secret"] {
        export_with(&temp, &manifest, None, |published| {
            published["dependencies"]["secret"] = json!(remote);
        })
        .expect("the hook pointed the dependency at a remote source");
    }
}

/// The check falls back to the workspace packages wherever the rewrite does:
/// an installed manifest without a version, or a missing relative target.
#[test]
fn follows_the_rewrite_to_the_workspace_packages() {
    let temp = fixture(true);
    fs::write(
        temp.path().join("app/node_modules/secret/package.json"),
        json!({ "name": "secret" }).to_string(),
    )
    .unwrap();
    fs::remove_dir_all(temp.path().join("secret")).unwrap();
    let packages = HashMap::from([(
        "secret".to_string(),
        WorkspacePackageManifest {
            name: "secret".to_string(),
            version: "1.0.0".to_string(),
            private: true,
        },
    )]);
    for spec in ["workspace:*", "workspace:../secret"] {
        assert_rejected(export(&temp, &app("dependencies", spec), Some(&packages)));
    }
}

/// A private package without a `version` is still private, even though the
/// rewrite cannot resolve `workspace:*` to it.
#[test]
fn rejects_a_private_workspace_package_without_a_version() {
    let temp = TempDir::new().expect("tempdir");
    let secret = json!({ "name": "secret", "private": true });
    let installed = temp.path().join("app/node_modules/secret");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join("package.json"), secret.to_string()).unwrap();
    assert_rejected(export(&temp, &app("dependencies", "workspace:^1.0.0"), None));

    fs::remove_dir_all(temp.path().join("app/node_modules")).unwrap();
    let packages = HashMap::from([(
        "secret".to_string(),
        WorkspacePackageManifest {
            name: "secret".to_string(),
            version: String::new(),
            private: true,
        },
    )]);
    assert_rejected(export(&temp, &app("dependencies", "workspace:^1.0.0"), Some(&packages)));
}

#[test]
fn rejects_a_private_workspace_package_a_hook_gives_a_new_range() {
    let temp = fixture(true);
    for spec in ["^1.0.0", "npm:secret@^1.0.0", "npm:1.0.0", "^1.0.0 || workspace:^2.0.0"] {
        assert_rejected(export_with(
            &temp,
            &app("dependencies", "workspace:*"),
            None,
            |published| published["dependencies"]["secret"] = json!(spec),
        ));
    }
}

#[test]
fn rejects_a_private_workspace_package_a_hook_moves_to_another_key() {
    let temp = fixture(true);
    assert_rejected(export_with(&temp, &app("dependencies", "workspace:*"), None, |published| {
        let dependencies = published["dependencies"].as_object_mut().unwrap();
        dependencies.remove("secret");
        dependencies.insert("renamed".to_string(), json!("npm:secret@1.0.0"));
    }));
}

#[test]
fn skips_the_bundled_dependencies_it_is_given() {
    let temp = fixture(true);
    let dir = temp.path().join("app");
    let catalogs = Catalogs::default();
    let opts = CreateExportableManifestOptions {
        catalogs: &catalogs,
        workspace_dir: None,
        modules_dir: None,
        skip_manifest_obfuscation: false,
        embed_readme: false,
        workspace_packages: None,
        bumped_workspace_packages: None,
    };
    let manifest = app("dependencies", "workspace:*");
    let private = private_workspace_dependencies(&dir, &manifest, &opts).expect("find private");
    let published = create_exportable_manifest(&dir, &manifest, &opts).expect("exportable");
    reject_private_workspace_dependencies(&published, &private, &["secret".to_string()])
        .expect("a bundled dependency ships in the tarball");
}
