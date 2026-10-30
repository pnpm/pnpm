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

    let subtable_manifest = "[dependencies.reqwest]\nversion = \"0.11\"\nfeatures = [\"json\"]\n";
    let updated_subtable =
        upsert_dependency(subtable_manifest, "dependencies", "reqwest", "=0.12.0").unwrap();
    assert_eq!(
        updated_subtable,
        "[dependencies.reqwest]\nversion = \"=0.12.0\"\nfeatures = [\"json\"]\n",
    );

    let single_quoted_subtable = "[dependencies.'reqwest']\nversion = \"0.11\"\n";
    let updated_single_quoted =
        upsert_dependency(single_quoted_subtable, "dependencies", "reqwest", "=0.12.0").unwrap();
    assert_eq!(updated_single_quoted, "[dependencies.'reqwest']\nversion = \"=0.12.0\"\n");

    let dotted_manifest =
        "[dependencies]\nreqwest.features = [\"json\"]\nreqwest.version = \"0.11\"\n";
    let updated_dotted =
        upsert_dependency(dotted_manifest, "dependencies", "reqwest", "=0.12.0").unwrap();
    assert_eq!(
        updated_dotted,
        "[dependencies]\nreqwest.features = [\"json\"]\nreqwest.version = \"=0.12.0\"\n",
    );
}

#[test]
fn updates_an_inline_table_that_spans_several_lines() {
    let manifest = "[workspace.dependencies]\naxum = { version = \"0.8.7\", default-features = false, features = [\n  \"http1\", # version = \"0\"\n  \"json\",\n] }\nbase64 = \"0.22\"\n";
    let updated = upsert_dependency(manifest, "workspace.dependencies", "axum", "0.8.9").unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    assert_eq!(
        updated,
        "[workspace.dependencies]\naxum = { version = \"0.8.9\", default-features = false, features = [\n  \"http1\", # version = \"0\"\n  \"json\",\n] }\nbase64 = \"0.22\"\n",
    );

    let version_last = "[dependencies]\ntokio = { features = [\n  \"rt\", # } version = \"0\"\n], version = \"1\" }\n";
    let updated = upsert_dependency(version_last, "dependencies", "tokio", "1.43").unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    assert_eq!(
        updated,
        "[dependencies]\ntokio = { features = [\n  \"rt\", # } version = \"0\"\n], version = \"1.43\" }\n",
    );

    let multi_line_strings = "[dependencies]\nlocal = { path = \'\'\'crates/it\'s}dep\'\'\', features = [\n  \"\"\"a \"quoted\"} version = \"0\"\"\"\"\",\n], version = \"1\" }\n";
    let updated = upsert_dependency(multi_line_strings, "dependencies", "local", "2").unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    assert_eq!(updated, multi_line_strings.replace(r#"version = "1""#, r#"version = "2""#));
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

    let repeated_glob_root = root.join("repeated-glob-ws");
    let deep_non_member =
        repeated_glob_root.join("a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z");
    fs::create_dir_all(&deep_non_member).unwrap();
    fs::write(
        repeated_glob_root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"**/**/**/**/**/**/**/**/**/never-matches\"]\n",
    )
    .unwrap();
    assert!(!workspace_includes_member(&repeated_glob_root, &deep_non_member));
}

#[test]
fn workspace_membership_discovers_implicit_path_dependency_members() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let member = root.join("crates/alpha");
    let implicit_lib = root.join("support/implicit-lib");
    let inherited_lib = root.join("support/inherited-lib");
    let excluded_lib = root.join("support/excluded-lib");
    for path in [&member, &implicit_lib, &inherited_lib, &excluded_lib] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("Cargo.toml"), "[package]\nname = \"x\"\nversion = \"0.1.0\"\n")
            .unwrap();
    }
    fs::write(
        implicit_lib.join("Cargo.toml"),
        "[package]\nname = \"implicit-lib\"\nversion = \"0.1.0\"\n\n[dependencies]\ninherited-alias = { workspace = true }\n",
    )
    .unwrap();
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n\n[dependencies]\nimplicit-lib = { path = \"../../support/implicit-lib\" }\nexcluded-lib = { path = \"../../support/excluded-lib\" }\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"support/excluded-lib\"]\n\n[workspace.dependencies]\ninherited-alias = { package = \"inherited-lib\", path = \"support/inherited-lib\" }\n",
    )
    .unwrap();

    assert!(workspace_includes_member(root, &implicit_lib));
    assert!(workspace_includes_member(root, &inherited_lib));
    assert!(!workspace_includes_member(root, &excluded_lib));
    assert_eq!(find_cargo_workspace_root(&implicit_lib, Some(root)), root);
    assert_eq!(find_cargo_workspace_root(&inherited_lib, Some(root)), root);
    assert_eq!(find_cargo_workspace_root(&excluded_lib, Some(root)), excluded_lib);
}
