use super::{
    CargoDependencyKind, upsert_dependency, workspace_dependency_spec, workspace_includes_member,
};
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

    let ws_dotted = "[workspace]\nmembers = [\"a\"]\ndependencies.serde = \"1.0\"\n";
    let updated_ws_dotted =
        upsert_dependency(ws_dotted, "workspace.dependencies", "itoa", "1.0.18").unwrap();
    assert_eq!(
        updated_ws_dotted,
        "[workspace]\nmembers = [\"a\"]\ndependencies.serde = \"1.0\"\ndependencies.itoa = \"1.0.18\"\n",
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
fn adds_dependencies_past_multiline_arrays_and_bracketed_lines() {
    let manifest = concat!(
        "[package]\n",
        "name = \"demo\"\n",
        "description = \"\"\"\n[not-a-section]\n\"\"\"\n\n",
        "[dependencies]\n",
        "serde = {\n",
        "  version = \"1.0\",\n",
        "  features =\n",
        "  [\"derive\"]\n",
        "}\n",
        "tokio = { version = \"1\", features = [\n",
        "  \"rt\"\n",
        "] }\n\n",
        "[features]\n",
        "default = []\n",
    );

    let updated = upsert_dependency(manifest, "dependencies", "itoa", "1.0.18").unwrap();
    let parsed: toml::Table =
        toml::from_str(&updated).expect("updated manifest must be valid TOML");
    assert_eq!(parsed["dependencies"]["itoa"].as_str(), Some("1.0.18"));
    assert_eq!(
        parsed["dependencies"]["serde"]["features"].as_array().map(Vec::as_slice),
        Some(["derive".into()].as_slice()),
    );
    assert_eq!(
        updated,
        concat!(
            "[package]\n",
            "name = \"demo\"\n",
            "description = \"\"\"\n[not-a-section]\n\"\"\"\n\n",
            "[dependencies]\n",
            "serde = {\n",
            "  version = \"1.0\",\n",
            "  features =\n",
            "  [\"derive\"]\n",
            "}\n",
            "tokio = { version = \"1\", features = [\n",
            "  \"rt\"\n",
            "] }\n\n",
            "itoa = \"1.0.18\"\n",
            "[features]\n",
            "default = []\n",
        ),
    );

    let subtable_array = concat!(
        "[dependencies.serde]\n",
        "features = [\n",
        "  \"derive\"\n",
        "]\n\n",
        "[features]\n",
        "default = []\n",
    );
    let updated_subtable =
        upsert_dependency(subtable_array, "dependencies", "serde", "1.0.228").unwrap();
    let parsed_subtable: toml::Table =
        toml::from_str(&updated_subtable).expect("updated subtable must be valid TOML");
    assert_eq!(parsed_subtable["dependencies"]["serde"]["version"].as_str(), Some("1.0.228"));
    assert_eq!(
        updated_subtable,
        concat!(
            "[dependencies.serde]\n",
            "features = [\n",
            "  \"derive\"\n",
            "]\n\n",
            "version = \"1.0.228\"\n",
            "[features]\n",
            "default = []\n",
        ),
    );
}

#[test]
fn replaces_workspace_inheritance_when_setting_an_explicit_version() {
    let inline_first = "[dependencies]\nfoo = { workspace = true, features = [\"a\"], optional = true, default-features = false, package = \"real-foo\" }\n";
    let updated_inline_first =
        upsert_dependency(inline_first, "dependencies", "foo", "1.0.0").unwrap();
    assert_eq!(
        updated_inline_first,
        "[dependencies]\nfoo = { version = \"1.0.0\", features = [\"a\"], optional = true, default-features = false, package = \"real-foo\" }\n",
    );
    let parsed: toml::Table = toml::from_str(&updated_inline_first).unwrap();
    assert_eq!(parsed["dependencies"]["foo"].get("workspace"), None);

    let inline_last = "[dependencies]\nfoo = { features = [\"a\"], workspace = true }\n";
    let updated_inline_last =
        upsert_dependency(inline_last, "dependencies", "foo", "1.0.0").unwrap();
    assert_eq!(
        updated_inline_last,
        "[dependencies]\nfoo = { features = [\"a\"], version = \"1.0.0\" }\n",
    );

    let inline_only = "[dependencies]\nfoo = { workspace = true }\n";
    assert_eq!(
        upsert_dependency(inline_only, "dependencies", "foo", "1.0.0").unwrap(),
        "[dependencies]\nfoo = { version = \"1.0.0\" }\n",
    );

    let inline_both =
        "[dependencies]\nfoo = { workspace = false, version = \"0.9\", features = [\"a\"] }\n";
    assert_eq!(
        upsert_dependency(inline_both, "dependencies", "foo", "1.0.0").unwrap(),
        "[dependencies]\nfoo = { version = \"1.0.0\", features = [\"a\"] }\n",
    );

    let subtable = concat!(
        "[dependencies.foo]\n",
        "workspace = true\n",
        "features = [\"a\"]\n",
        "optional = true\n",
        "default-features = false\n",
        "package = \"real-foo\"\n",
    );
    let updated_subtable = upsert_dependency(subtable, "dependencies", "foo", "1.0.0").unwrap();
    assert_eq!(
        updated_subtable,
        concat!(
            "[dependencies.foo]\n",
            "version = \"1.0.0\"\n",
            "features = [\"a\"]\n",
            "optional = true\n",
            "default-features = false\n",
            "package = \"real-foo\"\n",
        ),
    );
    let parsed_subtable: toml::Table = toml::from_str(&updated_subtable).unwrap();
    assert_eq!(parsed_subtable["dependencies"]["foo"].get("workspace"), None);

    let dotted = "[dependencies]\nfoo.workspace = true\nfoo.features = [\"a\"]\n";
    let updated_dotted = upsert_dependency(dotted, "dependencies", "foo", "1.0.0").unwrap();
    assert_eq!(updated_dotted, "[dependencies]\nfoo.version = \"1.0.0\"\nfoo.features = [\"a\"]\n");
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

    let by_alias = workspace_dependency_spec(&member, "inherited-alias", Some("inherited-lib"))
        .expect("resolve workspace dependency when both workspace and member spell package");
    assert_eq!(by_alias.package_name, "inherited-lib");
    assert_eq!(by_alias.path.as_deref(), Some("support/inherited-lib"));
}
