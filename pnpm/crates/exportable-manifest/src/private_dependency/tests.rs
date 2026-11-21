use super::PrivateWorkspaceDependencyError;
use crate::{
    CreateExportableManifestError,
    CreateExportableManifestOptions,
    WorkspacePackageManifest,
    create_exportable_manifest,
};
use pnpm_catalogs_types::Catalogs;
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

fn export(
    temp: &TempDir,
    manifest: &Value,
    workspace_packages: Option<&HashMap<String, WorkspacePackageManifest>>,
) -> Result<Value, CreateExportableManifestError> {
    let catalogs = Catalogs::default();
    create_exportable_manifest(
        &temp.path().join("app"),
        manifest,
        &CreateExportableManifestOptions {
            catalogs: &catalogs,
            workspace_dir: None,
            modules_dir: None,
            skip_manifest_obfuscation: false,
            embed_readme: false,
            workspace_packages,
            bumped_workspace_packages: None,
        },
    )
}

fn app(field: &str, spec: &str) -> Value {
    json!({ "name": "app", "version": "1.0.0", field: { "secret": spec } })
}

fn assert_rejected(result: Result<Value, CreateExportableManifestError>) {
    match result {
        Err(CreateExportableManifestError::PrivateWorkspaceDependency(
            PrivateWorkspaceDependencyError { package_name, dep_name },
        )) => assert_eq!((package_name.as_str(), dep_name.as_str()), ("app", "secret")),
        other => panic!("expected the private dependency error, got {other:?}"),
    }
}

#[test]
fn rejects_a_private_workspace_package_in_dependencies() {
    let temp = fixture(true);
    for spec in ["workspace:*", "workspace:^1.0.0", "workspace:secret@*", "workspace:../secret"] {
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
    let mut bundled = app("dependencies", "workspace:*");
    bundled["bundleDependencies"] = json!(["secret"]);
    let mut private = app("dependencies", "workspace:*");
    private["private"] = json!(true);
    for manifest in [
        app("devDependencies", "workspace:*"),
        app("optionalDependencies", "workspace:*"),
        app("peerDependencies", "workspace:*"),
        bundled,
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
