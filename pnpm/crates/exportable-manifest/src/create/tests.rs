use super::{
    CreateExportableManifestError, CreateExportableManifestOptions, create_exportable_manifest,
};
use pacquet_catalogs_types::{Catalog, Catalogs};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};
use tempfile::tempdir;

fn empty_catalogs() -> Catalogs {
    BTreeMap::new()
}

fn build(dir: &Path, manifest: &Value, opts: &CreateExportableManifestOptions<'_>) -> Value {
    create_exportable_manifest(dir, manifest, opts).unwrap()
}

fn default_opts(catalogs: &Catalogs) -> CreateExportableManifestOptions<'_> {
    CreateExportableManifestOptions {
        catalogs,
        modules_dir: None,
        workspace_versions: None,
        skip_manifest_obfuscation: false,
        embed_readme: false,
    }
}

/// Write a dependency's installed `package.json` under
/// `<dir>/node_modules/<name>/` so workspace-protocol rewriting can
/// resolve it.
fn install_dep(dir: &Path, name: &str, version: &str) {
    let dep_dir = dir.join("node_modules").join(name);
    fs::create_dir_all(&dep_dir).unwrap();
    fs::write(
        dep_dir.join("package.json"),
        serde_json::to_string(&json!({ "name": name, "version": version })).unwrap(),
    )
    .unwrap();
}

#[test]
fn obfuscation_strips_pnpm_internal_fields() {
    let dir = tempdir().unwrap();
    let catalogs = empty_catalogs();
    let out = build(
        dir.path(),
        &json!({
            "name": "foo",
            "version": "1.0.0",
            "packageManager": "pnpm@9.0.0",
            "pnpm": { "overrides": {} },
            "scripts": { "build": "tsc", "prepack": "x", "prepare": "y", "test": "jest" },
        }),
        &default_opts(&catalogs),
    );
    assert!(out.get("packageManager").is_none());
    assert!(out.get("pnpm").is_none());
    // Publish-lifecycle scripts are stripped; ordinary scripts stay.
    assert_eq!(out["scripts"], json!({ "build": "tsc", "test": "jest" }));
}

#[test]
fn skip_obfuscation_keeps_scripts_and_package_manager() {
    let dir = tempdir().unwrap();
    let catalogs = empty_catalogs();
    let opts = CreateExportableManifestOptions {
        catalogs: &catalogs,
        modules_dir: None,
        workspace_versions: None,
        skip_manifest_obfuscation: true,
        embed_readme: false,
    };
    let out = build(
        dir.path(),
        &json!({
            "name": "foo",
            "version": "1.0.0",
            "packageManager": "pnpm@9.0.0",
            "pnpm": { "overrides": {} },
            "scripts": { "prepack": "x", "build": "tsc" },
        }),
        &opts,
    );
    assert_eq!(out["packageManager"], json!("pnpm@9.0.0"));
    // Only the pnpm field is dropped under skip_manifest_obfuscation.
    assert!(out.get("pnpm").is_none());
    assert_eq!(out["scripts"], json!({ "prepack": "x", "build": "tsc" }));
}

#[test]
fn workspace_protocol_dependency_is_rewritten_to_installed_version() {
    let dir = tempdir().unwrap();
    install_dep(dir.path(), "bar", "2.3.4");
    let catalogs = empty_catalogs();
    let out = build(
        dir.path(),
        &json!({
            "name": "foo",
            "version": "1.0.0",
            "dependencies": { "bar": "workspace:^" },
        }),
        &default_opts(&catalogs),
    );
    assert_eq!(out["dependencies"], json!({ "bar": "^2.3.4" }));
}

#[test]
fn catalog_protocol_dependency_is_resolved() {
    let dir = tempdir().unwrap();
    let mut catalog = Catalog::new();
    catalog.insert("bar".to_string(), "^3.0.0".to_string());
    let mut catalogs = empty_catalogs();
    catalogs.insert("default".to_string(), catalog);
    let out = build(
        dir.path(),
        &json!({
            "name": "foo",
            "version": "1.0.0",
            "dependencies": { "bar": "catalog:" },
        }),
        &default_opts(&catalogs),
    );
    assert_eq!(out["dependencies"], json!({ "bar": "^3.0.0" }));
}

#[test]
fn jsr_protocol_dependency_becomes_npm_alias() {
    let dir = tempdir().unwrap();
    let catalogs = empty_catalogs();
    let out = build(
        dir.path(),
        &json!({
            "name": "foo",
            "version": "1.0.0",
            "dependencies": { "@foo/bar": "jsr:^1.2.3" },
        }),
        &default_opts(&catalogs),
    );
    assert_eq!(out["dependencies"], json!({ "@foo/bar": "npm:@jsr/foo__bar@^1.2.3" }));
}

#[test]
fn jsr_dependency_without_version_selector_becomes_bare_npm_alias() {
    let dir = tempdir().unwrap();
    let catalogs = empty_catalogs();
    let out = build(
        dir.path(),
        &json!({
            "name": "foo",
            "version": "1.0.0",
            "dependencies": { "@foo/bar": "jsr:@foo/bar" },
        }),
        &default_opts(&catalogs),
    );
    assert_eq!(out["dependencies"], json!({ "@foo/bar": "npm:@jsr/foo__bar" }));
}

#[test]
fn peer_workspace_protocol_dependency_is_rewritten() {
    let dir = tempdir().unwrap();
    install_dep(dir.path(), "bar", "3.0.0");
    let catalogs = empty_catalogs();
    let out = build(
        dir.path(),
        &json!({
            "name": "foo",
            "version": "1.0.0",
            "peerDependencies": { "bar": "workspace:^" },
        }),
        &default_opts(&catalogs),
    );
    assert_eq!(out["peerDependencies"], json!({ "bar": "^3.0.0" }));
}

#[test]
fn workspace_dependencies_use_in_memory_snapshot_versions() {
    let dir = tempdir().unwrap();
    let catalogs = empty_catalogs();
    let workspace_versions =
        HashMap::from([("core".to_string(), "0.0.0-preview-20260718000000".to_string())]);
    let out = build(
        dir.path(),
        &json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "core": "workspace:^",
                "coreAlias": "workspace:core@*",
            },
            "peerDependencies": { "core": "workspace:^1.0.0" },
        }),
        &CreateExportableManifestOptions {
            workspace_versions: Some(&workspace_versions),
            ..default_opts(&catalogs)
        },
    );
    assert_eq!(
        out["dependencies"],
        json!({
            "core": "0.0.0-preview-20260718000000",
            "coreAlias": "npm:core@0.0.0-preview-20260718000000",
        }),
    );
    assert_eq!(out["peerDependencies"], json!({ "core": "0.0.0-preview-20260718000000" }));
}

#[test]
fn publish_config_whitelisted_keys_are_hoisted() {
    let dir = tempdir().unwrap();
    let catalogs = empty_catalogs();
    let out = build(
        dir.path(),
        &json!({
            "name": "foo",
            "version": "1.0.0",
            "main": "src/index.ts",
            "publishConfig": { "main": "dist/index.js", "access": "public" },
        }),
        &default_opts(&catalogs),
    );
    // Whitelisted `main` overrides the root; non-whitelisted `access`
    // stays in a trimmed publishConfig.
    assert_eq!(out["main"], json!("dist/index.js"));
    assert_eq!(out["publishConfig"], json!({ "access": "public" }));
}

#[test]
fn publish_config_is_removed_when_emptied() {
    let dir = tempdir().unwrap();
    let catalogs = empty_catalogs();
    let out = build(
        dir.path(),
        &json!({
            "name": "foo",
            "version": "1.0.0",
            "publishConfig": { "types": "dist/index.d.ts" },
        }),
        &default_opts(&catalogs),
    );
    assert_eq!(out["types"], json!("dist/index.d.ts"));
    assert!(out.get("publishConfig").is_none());
}

#[test]
fn readme_is_embedded_when_requested() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# Hello").unwrap();
    let catalogs = empty_catalogs();
    let opts = CreateExportableManifestOptions {
        catalogs: &catalogs,
        modules_dir: None,
        workspace_versions: None,
        skip_manifest_obfuscation: false,
        embed_readme: true,
    };
    let out = build(dir.path(), &json!({ "name": "foo", "version": "1.0.0" }), &opts);
    assert_eq!(out["readme"], json!("# Hello"));
}

#[test]
fn readme_is_not_embedded_without_opt_in() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# Hello").unwrap();
    let catalogs = empty_catalogs();
    let out =
        build(dir.path(), &json!({ "name": "foo", "version": "1.0.0" }), &default_opts(&catalogs));
    assert!(out.get("readme").is_none());
}

#[cfg(unix)]
#[test]
fn readme_symlink_is_not_embedded() {
    // A symlinked README could point outside the project; it must be skipped so its
    // target's contents can't be leaked into the published manifest.
    let dir = tempdir().unwrap();
    let secret = tempdir().unwrap();
    fs::write(secret.path().join("secret"), "TOP SECRET").unwrap();
    std::os::unix::fs::symlink(secret.path().join("secret"), dir.path().join("README.md")).unwrap();
    let catalogs = empty_catalogs();
    let opts = CreateExportableManifestOptions {
        catalogs: &catalogs,
        modules_dir: None,
        workspace_versions: None,
        skip_manifest_obfuscation: false,
        embed_readme: true,
    };
    let out = build(dir.path(), &json!({ "name": "foo", "version": "1.0.0" }), &opts);
    assert!(out.get("readme").is_none());
}

#[test]
fn missing_name_surfaces_transform_error() {
    let dir = tempdir().unwrap();
    let catalogs = empty_catalogs();
    let err = create_exportable_manifest(
        dir.path(),
        &json!({ "version": "1.0.0" }),
        &default_opts(&catalogs),
    )
    .unwrap_err();
    assert!(matches!(err, CreateExportableManifestError::Transform(_)));
}
