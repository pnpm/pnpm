use super::declares_yarn_workspaces;
use crate::cli_args::package_manager::read_root_manifest_json;
use std::{fs, path::Path};

fn write_manifest(dir: &Path, contents: &str) {
    fs::write(dir.join("package.json"), contents).expect("write package.json");
}

fn declares_in(dir: &Path) -> bool {
    declares_yarn_workspaces(read_root_manifest_json(dir).as_ref())
}

#[test]
fn reports_a_manifest_declaring_workspace_patterns() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(dir.path(), r#"{"name":"x","workspaces":["packages/*"]}"#);
    assert!(declares_in(dir.path()));
}

/// An editor-written manifest may open with a UTF-8 BOM, which every other
/// manifest read in the CLI strips. The warning has to see the same field
/// those reads do.
#[test]
fn reports_workspace_patterns_through_a_utf8_bom() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(dir.path(), "\u{feff}{\"workspaces\":[\"packages/*\"]}");
    assert!(declares_in(dir.path()));
}

/// An empty array selects no project, so pnpm behaves the same with and
/// without it. pnpm 11 does not warn about it either.
#[test]
fn ignores_an_empty_workspaces_array() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(dir.path(), r#"{"workspaces":[]}"#);
    assert!(!declares_in(dir.path()));
}

/// Yarn's object spelling, and any other shape the field is given, is left
/// to the reader pnpm 11 has: neither version warns about it.
#[test]
fn tolerates_absent_malformed_and_non_array_manifests() {
    let dir = tempfile::tempdir().expect("create temp dir");
    assert!(!declares_in(dir.path()), "no manifest");

    write_manifest(dir.path(), "{ not json");
    assert!(!declares_in(dir.path()), "malformed manifest");

    write_manifest(dir.path(), r#"{"name":"x"}"#);
    assert!(!declares_in(dir.path()), "no workspaces field");

    write_manifest(dir.path(), r#"{"workspaces":{"packages":["packages/*"]}}"#);
    assert!(!declares_in(dir.path()), "object workspaces field");
}
