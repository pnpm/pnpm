use super::{CargoDependencyKind, upsert_dependency, workspace_includes_member};
use crate::cli_args::ecosystem_deps::cargo_manifest_ops::find_cargo_workspace_root;
use std::fs;

#[test]
fn adds_a_dependency_without_reformatting_the_manifest() {
    let manifest =
        "[package]\nname = \"app\"\n\n[dependencies]\n# retained\n\n[features]\ndefault = []\n";
    let updated =
        upsert_dependency(manifest, CargoDependencyKind::Normal.table(), "serde", "1.0.228")
            .unwrap();

    assert_eq!(
        updated,
        "[package]\nname = \"app\"\n\n[dependencies]\n# retained\n\nserde = \"1.0.228\"\n[features]\ndefault = []\n",
    );
}

#[test]
fn recognizes_commented_table_headers_and_boundaries() {
    let manifest = "[package]\nname = \"demo\"\n\n[dependencies] # direct crates\nold = \"1\"\n\n[features] # feature flags\ndefault = []\n";

    let updated = upsert_dependency(manifest, "dependencies", "new", "2").unwrap();

    assert_eq!(
        updated,
        "[package]\nname = \"demo\"\n\n[dependencies] # direct crates\nold = \"1\"\n\nnew = \"2\"\n[features] # feature flags\ndefault = []\n",
    );
}

#[test]
fn creates_the_selected_dependency_table() {
    let updated = upsert_dependency(
        "[package]\nname = \"app\"\n",
        CargoDependencyKind::Development.table(),
        "insta",
        "1",
    )
    .unwrap();

    assert_eq!(updated, "[package]\nname = \"app\"\n\n[dev-dependencies]\ninsta = \"1\"\n");
}

#[test]
fn updates_strings_and_inline_tables_without_losing_formatting() {
    let manifest = "[dependencies]\nserde = '1' # keep\ntokio = { features = [\"version\"], version = \"1\" }\n";
    let updated = upsert_dependency(manifest, "dependencies", "serde", "2").unwrap();
    let updated = upsert_dependency(&updated, "dependencies", "tokio", "~1.43").unwrap();

    assert_eq!(
        updated,
        "[dependencies]\nserde = \"2\" # keep\ntokio = { features = [\"version\"], version = \"~1.43\" }\n",
    );
}

#[test]
fn workspace_membership_respects_single_segment_globs_and_literal_bang_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let member = root.join("crates/alpha");
    let nested = root.join("crates/tools/nested");
    let bang_member = root.join("!special");
    let excluded = root.join("crates/excluded");
    for path in [&member, &nested, &bang_member, &excluded] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("Cargo.toml"), "[package]\nname = \"x\"\nversion = \"0.1.0\"\n")
            .unwrap();
    }
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\", \"!special\"]\nexclude = [\"crates/excluded\"]\n",
    )
    .unwrap();

    assert!(workspace_includes_member(root, &member));
    assert!(!workspace_includes_member(root, &nested));
    assert!(workspace_includes_member(root, &bang_member));
    assert!(!workspace_includes_member(root, &excluded));

    assert_eq!(find_cargo_workspace_root(&member, Some(root)), root);
    assert_eq!(find_cargo_workspace_root(&nested, Some(root)), nested);
    assert_eq!(find_cargo_workspace_root(&bang_member, Some(root)), root);
    assert_eq!(find_cargo_workspace_root(&excluded, Some(root)), excluded);
}
