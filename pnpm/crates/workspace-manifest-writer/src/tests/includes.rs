//! Writes to a workspace whose `pnpm-workspace.yaml` includes other files.

use super::{TempDir, UpdateWorkspaceManifestOptions, catalogs, fs, update_workspace_manifest};
use crate::{
    UpdateWorkspaceManifestError, set_allow_builds, set_overrides, update_workspace_setting,
};
use std::path::Path;

const ROOT: &str = "configDependencies:\n  policy: 1.0.0\ninclude:\n  ./catalog.yaml: [catalog]\n  ./legacy.yaml: [catalogs.react18]\n  ./deploy.yaml: [nodeLinker]\n  ./security.yaml: '*'\n  policy: [minimumReleaseAge, minimumReleaseAgeExclude]\n";

fn workspace() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    for (name, text) in [
        ("pnpm-workspace.yaml", ROOT),
        ("catalog.yaml", "catalog:\n  react: ^19.0.0\n"),
        ("legacy.yaml", ""),
        ("deploy.yaml", "# deployment settings\n"),
        ("security.yaml", "allowBuilds:\n  esbuild: true\n"),
    ] {
        fs::write(dir.path().join(name), text).expect("seed file");
    }
    dir
}

fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name)).expect("read file")
}

#[test]
fn catalog_entries_go_to_the_file_owning_each_catalog() {
    let dir = workspace();
    let updated = catalogs(&[
        ("default", &[("react-dom", "^19.0.0")]),
        ("react18", &[("react", "^18.3.1")]),
        ("tools", &[("tsx", "^4.0.0")]),
    ]);
    let opts =
        UpdateWorkspaceManifestOptions { updated_catalogs: Some(&updated), ..Default::default() };
    update_workspace_manifest(dir.path(), &opts).expect("update");
    assert_eq!(
        read(dir.path(), "catalog.yaml"),
        "catalog:\n  react: ^19.0.0\n  react-dom: ^19.0.0\n",
    );
    assert_eq!(read(dir.path(), "legacy.yaml"), "catalogs:\n  react18:\n    react: ^18.3.1\n");
    assert_eq!(
        read(dir.path(), "pnpm-workspace.yaml"),
        format!("catalogs:\n  tools:\n    tsx: ^4.0.0\n{ROOT}"),
    );
}

#[test]
fn an_omitted_delegated_setting_is_written_to_its_owner() {
    let dir = workspace();
    update_workspace_setting(dir.path(), "nodeLinker", &"hoisted".into()).expect("set");
    assert_eq!(read(dir.path(), "deploy.yaml"), "nodeLinker: hoisted\n# deployment settings\n");
    assert_eq!(read(dir.path(), "pnpm-workspace.yaml"), ROOT);
}

#[test]
fn a_setting_defined_in_an_unrestricted_file_stays_there() {
    let dir = workspace();
    set_allow_builds(dir.path(), [("sharp", true)]).expect("set");
    assert_eq!(read(dir.path(), "security.yaml"), "allowBuilds:\n  esbuild: true\n  sharp: true\n");
}

#[test]
fn an_unowned_setting_is_added_to_the_root() {
    let dir = workspace();
    set_overrides(dir.path(), [("foo", "1.0.0")]).expect("set");
    assert_eq!(
        read(dir.path(), "pnpm-workspace.yaml"),
        format!("{ROOT}overrides:\n  foo: 1.0.0\n"),
    );
    assert_eq!(read(dir.path(), "security.yaml"), "allowBuilds:\n  esbuild: true\n");
}

#[test]
fn a_package_owned_setting_cannot_be_written() {
    let dir = workspace();
    let error = update_workspace_setting(dir.path(), "minimumReleaseAge", &0.into())
        .expect_err("package-owned");
    assert!(
        matches!(&error, UpdateWorkspaceManifestError::OwnedByConfigDependency { name, .. } if name == "policy"),
        "{error:?}",
    );
    let added = ["foo".to_string()];
    let opts = UpdateWorkspaceManifestOptions {
        added_minimum_release_age_excludes: &added,
        ..Default::default()
    };
    let error = update_workspace_manifest(dir.path(), &opts).expect_err("package-owned");
    assert!(
        matches!(error, UpdateWorkspaceManifestError::OwnedByConfigDependency { .. }),
        "{error:?}",
    );
}

#[test]
fn deleting_the_last_setting_of_an_included_file_keeps_the_file() {
    let dir = workspace();
    update_workspace_setting(dir.path(), "allowBuilds", &serde_json::Value::Null).expect("delete");
    assert_eq!(read(dir.path(), "security.yaml"), "");
    assert_eq!(read(dir.path(), "pnpm-workspace.yaml"), ROOT);
}

#[test]
fn an_invalid_include_index_fails_the_write() {
    let dir = workspace();
    fs::remove_file(dir.path().join("deploy.yaml")).expect("remove included file");
    let error = update_workspace_setting(dir.path(), "overrides", &serde_json::json!({}))
        .expect_err("missing include");
    assert!(matches!(error, UpdateWorkspaceManifestError::Include(_)), "{error:?}");
    assert_eq!(read(dir.path(), "pnpm-workspace.yaml"), ROOT);
}

#[test]
fn the_include_index_cannot_be_written() {
    let dir = workspace();
    let error = update_workspace_setting(dir.path(), "include", &serde_json::Value::Null)
        .expect_err("include is not writable");
    assert!(matches!(error, UpdateWorkspaceManifestError::IncludeNotWritable), "{error:?}");
    assert_eq!(read(dir.path(), "pnpm-workspace.yaml"), ROOT);
}

#[test]
fn catalogs_split_across_files_cannot_be_written_as_a_whole() {
    let dir = workspace();
    let error =
        update_workspace_setting(dir.path(), "catalogs", &serde_json::json!({ "tools": {} }))
            .expect_err("split catalogs");
    assert!(matches!(error, UpdateWorkspaceManifestError::CatalogsSplitAcrossFiles), "{error:?}");
}

#[cfg(unix)]
#[test]
fn a_symlinked_included_file_is_written_through_the_link() {
    let dir = workspace();
    fs::rename(dir.path().join("deploy.yaml"), dir.path().join("deploy-target.yaml"))
        .expect("move included file");
    std::os::unix::fs::symlink("deploy-target.yaml", dir.path().join("deploy.yaml"))
        .expect("symlink included file");
    update_workspace_setting(dir.path(), "nodeLinker", &"hoisted".into()).expect("set");
    assert!(fs::symlink_metadata(dir.path().join("deploy.yaml")).expect("stat").is_symlink());
    assert_eq!(
        read(dir.path(), "deploy-target.yaml"),
        "nodeLinker: hoisted\n# deployment settings\n",
    );
}

#[test]
fn a_rejected_edit_in_an_included_file_leaves_every_file_unchanged() {
    let dir = workspace();
    let legacy = "catalogs: {\n  react18: { react: ^18.0.0 }, # pinned\n}\n";
    fs::write(dir.path().join("legacy.yaml"), legacy).expect("seed file");
    let updated =
        catalogs(&[("tools", &[("tsx", "^4.0.0")]), ("react18", &[("react", "^18.3.1")])]);
    let opts =
        UpdateWorkspaceManifestOptions { updated_catalogs: Some(&updated), ..Default::default() };
    let error = update_workspace_manifest(dir.path(), &opts).expect_err("inline catalogs");
    dbg!(&error);
    assert!(matches!(error, UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    assert_eq!(read(dir.path(), "pnpm-workspace.yaml"), ROOT);
    assert_eq!(read(dir.path(), "legacy.yaml"), legacy);
}

#[cfg(unix)]
#[test]
fn a_failed_write_restores_the_files_written_before_it() {
    use std::os::unix::fs::PermissionsExt;

    let dir = workspace();
    let legacy_dir = dir.path().join("legacy");
    fs::create_dir(&legacy_dir).expect("create legacy dir");
    fs::write(legacy_dir.join("catalog.yaml"), "").expect("seed file");
    let root = ROOT.replace("./legacy.yaml", "./legacy/catalog.yaml");
    fs::write(dir.path().join("pnpm-workspace.yaml"), &root).expect("seed root");
    fs::set_permissions(&legacy_dir, fs::Permissions::from_mode(0o555)).expect("read-only dir");
    let updated =
        catalogs(&[("tools", &[("tsx", "^4.0.0")]), ("react18", &[("react", "^18.3.1")])]);
    let opts =
        UpdateWorkspaceManifestOptions { updated_catalogs: Some(&updated), ..Default::default() };
    let result = update_workspace_manifest(dir.path(), &opts);
    fs::set_permissions(&legacy_dir, fs::Permissions::from_mode(0o755)).expect("writable dir");

    let error = result.expect_err("the included file's directory is read-only");
    dbg!(&error);
    assert!(matches!(error, UpdateWorkspaceManifestError::Write { .. }));
    assert_eq!(read(dir.path(), "pnpm-workspace.yaml"), root);
}

#[test]
fn a_patch_path_is_written_relative_to_the_file_owning_patched_dependencies() {
    let dir = TempDir::new().expect("temp dir");
    fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "include:\n  ./patches/patches.yaml: [patchedDependencies]\n",
    )
    .expect("seed root");
    fs::create_dir(dir.path().join("patches")).expect("create patches dir");
    fs::write(dir.path().join("patches/patches.yaml"), "").expect("seed patches file");
    let patches =
        std::iter::once(("foo@1.0.0".to_string(), "patches/foo@1.0.0.patch".to_string())).collect();
    crate::set_patched_dependencies(dir.path(), &patches).expect("set");
    assert_eq!(
        read(dir.path(), "patches/patches.yaml"),
        "patchedDependencies:\n  foo@1.0.0: foo@1.0.0.patch\n",
    );
}

#[test]
fn a_local_catalog_entry_is_written_relative_to_the_file_owning_the_catalog() {
    let dir = TempDir::new().expect("temp dir");
    fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "include:\n  ./config/catalog.yaml: [catalog]\n",
    )
    .expect("seed root");
    fs::create_dir(dir.path().join("config")).expect("create config dir");
    fs::write(dir.path().join("config/catalog.yaml"), "").expect("seed catalog file");
    let updated = catalogs(&[("default", &[("local", "link:libs/local"), ("react", "^19.0.0")])]);
    let opts =
        UpdateWorkspaceManifestOptions { updated_catalogs: Some(&updated), ..Default::default() };
    update_workspace_manifest(dir.path(), &opts).expect("update");
    assert_eq!(
        read(dir.path(), "config/catalog.yaml"),
        "catalog:\n  local: link:../libs/local\n  react: ^19.0.0\n",
    );
}

#[test]
fn a_path_set_on_an_included_file_keeps_meaning_what_it_means_in_the_root() {
    let dir = TempDir::new().expect("temp dir");
    fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "include:\n  ./config/settings.yaml: [storeDir, nodeLinker]\n",
    )
    .expect("seed root");
    fs::create_dir(dir.path().join("config")).expect("create config dir");
    fs::write(dir.path().join("config/settings.yaml"), "").expect("seed settings file");
    update_workspace_setting(dir.path(), "storeDir", &"./store".into()).expect("set storeDir");
    update_workspace_setting(dir.path(), "nodeLinker", &"hoisted".into()).expect("set nodeLinker");
    assert_eq!(
        read(dir.path(), "config/settings.yaml"),
        "nodeLinker: hoisted\nstoreDir: ../store\n",
    );
}
