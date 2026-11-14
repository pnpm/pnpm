mod inheritance;

use super::{CargoDependencyKind, upsert_dependency};

#[test]
fn adds_a_dependency_without_reformatting_the_manifest() {
    let manifest =
        "[package]\nname = \"app\"\n\n[dependencies]\n# retained\n\n[features]\ndefault = []\n";
    let updated =
        upsert_dependency(manifest, CargoDependencyKind::Normal.table(), "serde", "1.0.228")
            .unwrap();
    eprintln!("UPDATED:\n{updated}\n");

    assert_eq!(
        updated,
        "[package]\nname = \"app\"\n\n[dependencies]\n# retained\n\nserde = \"1.0.228\"\n[features]\ndefault = []\n",
    );
}

#[test]
fn recognizes_commented_table_headers_and_boundaries() {
    let manifest = "[package]\nname = \"demo\"\n\n[dependencies] # direct crates\nold = \"1\"\n\n[features] # feature flags\ndefault = []\n";

    let updated = upsert_dependency(manifest, "dependencies", "new", "2").unwrap();
    eprintln!("UPDATED:\n{updated}\n");

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
    eprintln!("UPDATED:\n{updated}\n");

    assert_eq!(updated, "[package]\nname = \"app\"\n\n[dev-dependencies]\ninsta = \"1\"\n");
}

#[test]
fn updates_strings_and_inline_tables_without_losing_formatting() {
    let manifest = "[dependencies]\nserde = '1' # keep\ntokio = { features = [\"version\"], version = \"1\" }\n";
    let updated = upsert_dependency(manifest, "dependencies", "serde", "2").unwrap();
    let updated = upsert_dependency(&updated, "dependencies", "tokio", "~1.43").unwrap();
    eprintln!("UPDATED:\n{updated}\n");

    assert_eq!(
        updated,
        "[dependencies]\nserde = \"2\" # keep\ntokio = { features = [\"version\"], version = \"~1.43\" }\n",
    );

    let subtable_manifest = "[dependencies.reqwest]\nversion = \"0.11\"\nfeatures = [\"json\"]\n";
    let updated_subtable =
        upsert_dependency(subtable_manifest, "dependencies", "reqwest", "=0.12.0").unwrap();
    eprintln!("UPDATED:\n{updated_subtable}\n");
    assert_eq!(
        updated_subtable,
        "[dependencies.reqwest]\nversion = \"=0.12.0\"\nfeatures = [\"json\"]\n",
    );

    let single_quoted_subtable = "[dependencies.'reqwest']\nversion = \"0.11\"\n";
    let updated_single_quoted =
        upsert_dependency(single_quoted_subtable, "dependencies", "reqwest", "=0.12.0").unwrap();
    eprintln!("UPDATED:\n{updated_single_quoted}\n");
    assert_eq!(updated_single_quoted, "[dependencies.'reqwest']\nversion = \"=0.12.0\"\n");

    let dotted_manifest =
        "[dependencies]\nreqwest.features = [\"json\"]\nreqwest.version = \"0.11\"\n";
    let updated_dotted =
        upsert_dependency(dotted_manifest, "dependencies", "reqwest", "=0.12.0").unwrap();
    eprintln!("UPDATED:\n{updated_dotted}\n");
    assert_eq!(
        updated_dotted,
        "[dependencies]\nreqwest.features = [\"json\"]\nreqwest.version = \"=0.12.0\"\n",
    );

    let ws_dotted = "[workspace]\nmembers = [\"a\"]\ndependencies.serde = \"1.0\"\n";
    let updated_ws_dotted =
        upsert_dependency(ws_dotted, "workspace.dependencies", "itoa", "1.0.18").unwrap();
    eprintln!("UPDATED:\n{updated_ws_dotted}\n");
    assert_eq!(
        updated_ws_dotted,
        "[workspace]\nmembers = [\"a\"]\ndependencies.serde = \"1.0\"\ndependencies.itoa = \"1.0.18\"\n",
    );
}

#[test]
fn inserts_dotted_dependencies_before_explicit_child_headers() {
    let manifest = "dependencies.serde = \"1\"\n\n[dependencies.tokio]\nversion = \"1\"\n";
    let updated = upsert_dependency(manifest, "dependencies", "itoa", "1.0.18").unwrap();
    eprintln!("UPDATED:\n{updated}\n");
    let parsed: toml::Table = toml::from_str(&updated).unwrap();
    assert_eq!(parsed["dependencies"]["serde"].as_str(), Some("1"));
    assert_eq!(parsed["dependencies"]["itoa"].as_str(), Some("1.0.18"));
    assert_eq!(parsed["dependencies"]["tokio"]["version"].as_str(), Some("1"));
    assert_eq!(
        updated,
        "dependencies.serde = \"1\"\ndependencies.itoa = \"1.0.18\"\n\n[dependencies.tokio]\nversion = \"1\"\n",
    );
}

#[test]
fn updates_dotted_keys_inside_inline_dependency_tables() {
    let path_only = "dependencies = { foo.path = \"foo\" }\n\n[package]\nname = \"app\"\n";
    let updated_path = upsert_dependency(path_only, "dependencies", "foo", "1.0.0").unwrap();
    eprintln!("UPDATED:\n{updated_path}\n");
    assert_eq!(
        updated_path,
        "dependencies = { foo.version = \"1.0.0\", foo.path = \"foo\" }\n\n[package]\nname = \"app\"\n",
    );
    let parsed_path: toml::Table = toml::from_str(&updated_path).unwrap();
    assert_eq!(parsed_path["dependencies"]["foo"]["version"].as_str(), Some("1.0.0"));
    assert_eq!(parsed_path["dependencies"]["foo"]["path"].as_str(), Some("foo"));

    let ws_dotted = "dependencies = { foo.workspace = true, foo.optional = true }\n\n[package]\nname = \"app\"\n";
    let updated_ws = upsert_dependency(ws_dotted, "dependencies", "foo", "1.0.0").unwrap();
    eprintln!("UPDATED:\n{updated_ws}\n");
    assert_eq!(
        updated_ws,
        "dependencies = { foo.version = \"1.0.0\", foo.optional = true }\n\n[package]\nname = \"app\"\n",
    );

    let both_dotted = "dependencies = { foo.version = \"0.9\", foo.workspace = true }\n\n[package]\nname = \"app\"\n";
    let updated_both = upsert_dependency(both_dotted, "dependencies", "foo", "1.0.0").unwrap();
    eprintln!("UPDATED:\n{updated_both}\n");
    assert_eq!(
        updated_both,
        "dependencies = { foo.version = \"1.0.0\" }\n\n[package]\nname = \"app\"\n",
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
    eprintln!("UPDATED:\n{updated}\n");
    let parsed: toml::Table =
        toml::from_str(&updated).expect("updated manifest must be valid TOML");
    assert_eq!(parsed["dependencies"]["itoa"].as_str(), Some("1.0.18"));
    assert_eq!(
        parsed["dependencies"]["serde"]["features"].as_array().map(Vec::as_slice),
        Some(["derive".into()].as_slice()),
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
    eprintln!("UPDATED:\n{updated_subtable}\n");
    let parsed_subtable: toml::Table =
        toml::from_str(&updated_subtable).expect("updated subtable must be valid TOML");
    assert_eq!(parsed_subtable["dependencies"]["serde"]["version"].as_str(), Some("1.0.228"));
}

#[test]
fn replaces_workspace_inheritance_when_setting_an_explicit_version() {
    let inline_first = "[dependencies]\nfoo = { workspace = true, features = [\"a\"], optional = true, default-features = false, package = \"real-foo\" }\n";
    let updated_inline_first =
        upsert_dependency(inline_first, "dependencies", "foo", "1.0.0").unwrap();
    eprintln!("UPDATED:\n{updated_inline_first}\n");
    assert_eq!(
        updated_inline_first,
        "[dependencies]\nfoo = { version = \"1.0.0\", features = [\"a\"], optional = true, default-features = false, package = \"real-foo\" }\n",
    );

    let inline_last = "[dependencies]\nfoo = { features = [\"a\"], workspace = true }\n";
    let updated_inline_last =
        upsert_dependency(inline_last, "dependencies", "foo", "1.0.0").unwrap();
    eprintln!("UPDATED:\n{updated_inline_last}\n");
    assert_eq!(
        updated_inline_last,
        "[dependencies]\nfoo = { features = [\"a\"], version = \"1.0.0\" }\n",
    );

    let inline_both =
        "[dependencies]\nfoo = { workspace = false, version = \"0.9\", features = [\"a\"] }\n";
    let updated_inline_both =
        upsert_dependency(inline_both, "dependencies", "foo", "1.0.0").unwrap();
    eprintln!("UPDATED:\n{updated_inline_both}\n");
    assert_eq!(
        updated_inline_both,
        "[dependencies]\nfoo = { version = \"1.0.0\", features = [\"a\"] }\n",
    );

    let subtable = "[dependencies.foo]\nworkspace = true\nfeatures = [\"a\"]\noptional = true\n";
    let updated_subtable = upsert_dependency(subtable, "dependencies", "foo", "1.0.0").unwrap();
    eprintln!("UPDATED:\n{updated_subtable}\n");
    assert_eq!(
        updated_subtable,
        "[dependencies.foo]\nversion = \"1.0.0\"\nfeatures = [\"a\"]\noptional = true\n",
    );

    let dotted = "[dependencies]\nfoo.workspace = true\nfoo.features = [\"a\"]\n";
    let updated_dotted = upsert_dependency(dotted, "dependencies", "foo", "1.0.0").unwrap();
    eprintln!("UPDATED:\n{updated_dotted}\n");
    assert_eq!(updated_dotted, "[dependencies]\nfoo.version = \"1.0.0\"\nfoo.features = [\"a\"]\n");
}
