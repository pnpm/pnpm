use super::super::{
    InvalidWorkspaceManifestError, ReadWorkspaceManifestError, WORKSPACE_MANIFEST_FILENAME,
    read_workspace_manifest,
};
use pnpm_catalogs_types::{Catalog, Catalogs};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};
use tempfile::TempDir;

fn write_manifest(dir: &Path, yaml: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(WORKSPACE_MANIFEST_FILENAME), yaml).unwrap();
}

fn inherited(dir: &Path) -> Catalogs {
    read_workspace_manifest(dir).unwrap().expect("the manifest exists").inherited_catalogs
}

fn catalog(entries: &[(&str, &str)]) -> Catalog {
    entries
        .iter()
        .map(|(name, spec)| ((*name).to_string(), (*spec).to_string()))
        .collect()
}

fn default_catalog(entries: &[(&str, &str)]) -> Catalogs {
    Catalogs::from([("default".to_string(), catalog(entries))])
}

#[test]
fn inherits_the_catalogs_of_an_extended_directory() {
    let tmp = TempDir::new().unwrap();
    write_manifest(
        &tmp.path().join("shared"),
        "catalog:\n  foo: ^1.0.0\ncatalogs:\n  react18:\n    react: ^18.0.0\n",
    );
    write_manifest(tmp.path(), "extends: ./shared\n");

    assert_eq!(
        inherited(tmp.path()),
        Catalogs::from([
            ("default".to_string(), catalog(&[("foo", "^1.0.0")])),
            ("react18".to_string(), catalog(&[("react", "^18.0.0")])),
        ]),
    );
}

#[test]
fn a_manifest_named_later_wins_entry_by_entry() {
    let tmp = TempDir::new().unwrap();
    write_manifest(&tmp.path().join("a"), "catalog:\n  foo: 1.0.0\n  bar: 1.0.0\n");
    write_manifest(&tmp.path().join("b"), "catalog:\n  foo: 2.0.0\n");
    write_manifest(tmp.path(), "extends:\n  - ./a\n  - ./b\n");

    assert_eq!(inherited(tmp.path()), default_catalog(&[("bar", "1.0.0"), ("foo", "2.0.0")]));
}

#[test]
fn an_extended_manifest_passes_on_what_it_inherits_under_its_own_entries() {
    let tmp = TempDir::new().unwrap();
    write_manifest(&tmp.path().join("base"), "catalog:\n  foo: 1.0.0\n  bar: 1.0.0\n");
    write_manifest(&tmp.path().join("middle"), "extends: ../base\ncatalog:\n  foo: 2.0.0\n");
    write_manifest(tmp.path(), "extends: middle\n");

    assert_eq!(inherited(tmp.path()), default_catalog(&[("bar", "1.0.0"), ("foo", "2.0.0")]));
}

#[test]
fn a_reference_may_name_the_manifest_file_itself_outside_the_workspace() {
    let shared = TempDir::new().unwrap();
    write_manifest(shared.path(), "catalog:\n  foo: 1.0.0\n");
    let workspace = TempDir::new().unwrap();
    let reference = shared.path().join(WORKSPACE_MANIFEST_FILENAME);
    write_manifest(workspace.path(), &format!("extends: '{}'\n", reference.display()));

    assert_eq!(inherited(workspace.path()), default_catalog(&[("foo", "1.0.0")]));
}

#[test]
fn a_glob_merges_every_matching_manifest_in_path_order() {
    let tmp = TempDir::new().unwrap();
    write_manifest(&tmp.path().join("packages/a"), "catalog:\n  foo: 1.0.0\n  bar: 1.0.0\n");
    write_manifest(&tmp.path().join("packages/b"), "catalog:\n  foo: 2.0.0\n");
    fs::create_dir_all(tmp.path().join("packages/without-manifest")).unwrap();
    write_manifest(&tmp.path().join("packages/node_modules/c"), "catalog:\n  foo: 3.0.0\n");
    write_manifest(tmp.path(), "extends: packages/*\n");

    assert_eq!(inherited(tmp.path()), default_catalog(&[("bar", "1.0.0"), ("foo", "2.0.0")]));
}

#[test]
fn a_glob_under_a_missing_directory_matches_nothing() {
    let tmp = TempDir::new().unwrap();
    write_manifest(tmp.path(), "extends: missing/*\n");

    assert_eq!(inherited(tmp.path()), Catalogs::new());
}

#[test]
fn a_reference_to_a_directory_without_a_manifest_is_an_error() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("shared")).unwrap();
    write_manifest(tmp.path(), "extends: ./shared\n");

    let error = read_workspace_manifest(tmp.path()).unwrap_err();
    dbg!(&error);
    assert!(matches!(
        error,
        ReadWorkspaceManifestError::ExtendsNotFound { ref dir, .. } if dir.ends_with("shared")
    ));
}

#[test]
fn a_manifest_extending_itself_through_another_is_an_error() {
    let tmp = TempDir::new().unwrap();
    write_manifest(&tmp.path().join("packages/a"), "extends: ../..\n");
    write_manifest(tmp.path(), "extends: packages/*\n");

    let error = read_workspace_manifest(tmp.path()).unwrap_err();
    dbg!(&error);
    assert!(matches!(error, ReadWorkspaceManifestError::ExtendsCycle { .. }));
}

#[test]
fn a_manifest_extending_itself_is_an_error() {
    let tmp = TempDir::new().unwrap();
    write_manifest(tmp.path(), "extends: .\n");

    let error = read_workspace_manifest(tmp.path()).unwrap_err();
    dbg!(&error);
    assert!(matches!(error, ReadWorkspaceManifestError::ExtendsCycle { .. }));
}

#[test]
fn a_manifest_reached_twice_without_a_cycle_is_merged() {
    let tmp = TempDir::new().unwrap();
    write_manifest(&tmp.path().join("base"), "catalog:\n  foo: 1.0.0\n");
    write_manifest(&tmp.path().join("a"), "extends: ../base\n");
    write_manifest(&tmp.path().join("b"), "extends: ../base\n");
    write_manifest(tmp.path(), "extends: [./a, ./b]\n");

    assert_eq!(inherited(tmp.path()), default_catalog(&[("foo", "1.0.0")]));
}

#[test]
fn an_empty_reference_is_invalid() {
    let tmp = TempDir::new().unwrap();
    write_manifest(tmp.path(), "extends: ['']\n");

    let error = read_workspace_manifest(tmp.path()).unwrap_err();
    dbg!(&error);
    assert!(matches!(
        error,
        ReadWorkspaceManifestError::Invalid(InvalidWorkspaceManifestError::EmptyExtendsEntry)
    ));
}

#[test]
fn an_extended_manifest_declaring_the_default_catalog_twice_is_an_error() {
    let tmp = TempDir::new().unwrap();
    write_manifest(
        &tmp.path().join("shared"),
        "catalog:\n  foo: 1.0.0\ncatalogs:\n  default:\n    foo: 2.0.0\n",
    );
    write_manifest(tmp.path(), "extends: ./shared\n");

    let error = read_workspace_manifest(tmp.path()).unwrap_err();
    dbg!(&error);
    assert!(matches!(error, ReadWorkspaceManifestError::ExtendedDefaultCatalogDefinedTwice { .. }));
}

#[test]
fn a_manifest_without_extends_inherits_nothing() {
    let tmp = TempDir::new().unwrap();
    write_manifest(tmp.path(), "catalog:\n  foo: 1.0.0\n");

    assert_eq!(inherited(tmp.path()), Catalogs::new());
}

#[test]
fn inherited_local_paths_stay_relative_to_the_manifest_declaring_them() {
    let tmp = TempDir::new().unwrap();
    write_manifest(
        &tmp.path().join("shared"),
        "catalog:\n  local: file:./vendor/local\n  linked: link:../linked\n  foo: ^1.0.0\n",
    );
    write_manifest(tmp.path(), "extends: ./shared\n");

    assert_eq!(
        inherited(tmp.path()),
        default_catalog(&[
            ("foo", "^1.0.0"),
            ("linked", "link:linked"),
            ("local", "file:shared/vendor/local"),
        ]),
    );
}

#[test]
fn a_glob_may_name_the_manifest_file_itself() {
    let tmp = TempDir::new().unwrap();
    write_manifest(&tmp.path().join("configs/a"), "catalog:\n  foo: 1.0.0\n");
    write_manifest(&tmp.path().join("configs/b"), "catalog:\n  bar: 1.0.0\n");
    write_manifest(tmp.path(), "extends: configs/*/pnpm-workspace.yaml\n");

    assert_eq!(inherited(tmp.path()), default_catalog(&[("bar", "1.0.0"), ("foo", "1.0.0")]));
}

#[test]
fn an_invalid_glob_is_an_error() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("packages")).unwrap();
    write_manifest(tmp.path(), "extends: 'packages/[a'\n");

    let error = read_workspace_manifest(tmp.path()).unwrap_err();
    dbg!(&error);
    assert!(matches!(
        error,
        ReadWorkspaceManifestError::Invalid(
            InvalidWorkspaceManifestError::InvalidExtendsPattern { .. }
        )
    ));
}

#[cfg(unix)]
#[test]
fn a_manifest_extending_itself_through_a_symlink_is_an_error() {
    let tmp = TempDir::new().unwrap();
    std::os::unix::fs::symlink(".", tmp.path().join("link")).unwrap();
    write_manifest(tmp.path(), "extends: ./link\n");

    let error = read_workspace_manifest(tmp.path()).unwrap_err();
    dbg!(&error);
    assert!(matches!(error, ReadWorkspaceManifestError::ExtendsCycle { .. }));
}
