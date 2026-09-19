use super::{catalogs, project, run, run_cleanup};

#[test]
fn pruning_an_anchor_promotes_the_first_surviving_alias() {
    let original = "catalog:\n  react: &react '^1.0.0' # definition\n  react-dom: *react # consumer\n  react-is: *react\n";
    let manifest = project(serde_json::json!({
        "dependencies": { "react-dom": "catalog:", "react-is": "catalog:" },
    }));
    let output = run_cleanup(Some(original), None, &[&manifest]).unwrap();
    assert_eq!(output, "catalog:\n  react-dom: &react '^1.0.0' # consumer\n  react-is: *react\n");
}

#[test]
fn updating_anchor_does_not_change_unchanged_aliases() {
    let original = "catalog:\n  react: &react ^1.0.0\n  react-dom: *react\n";
    let output = run(Some(original), &catalogs(&[("default", &[("react", "^2.0.0")])])).unwrap();
    assert_eq!(output, "catalog:\n  react: &react ^2.0.0\n  react-dom: ^1.0.0\n");
}

#[test]
fn updates_flow_catalogs_and_preserves_comments() {
    let original = "catalog: {react: &react ^1.0.0, react-dom: *react} # shared\n";
    let output = run(
        Some(original),
        &catalogs(&[("default", &[("react", "^2.0.0"), ("react-dom", "^2.0.0")])]),
    )
    .unwrap();
    assert_eq!(output, "catalog: { react: &react ^2.0.0, react-dom: *react } # shared\n");
}

#[test]
fn aliases_can_cross_named_catalogs() {
    let original =
        "catalogs:\n  first:\n    react: &react ^1.0.0\n  second:\n    react-dom: *react\n";
    let output = run(
        Some(original),
        &catalogs(&[("first", &[("react", "^2.0.0")]), ("second", &[("react-dom", "^2.0.0")])]),
    )
    .unwrap();
    assert_eq!(
        output,
        "catalogs:\n  first:\n    react: &react ^2.0.0\n  second:\n    react-dom: *react\n"
    );
}

#[test]
fn pruning_catalog_preserves_aliases_in_other_settings() {
    let original = "catalog:\n  unused: &version ^1.0.0\noverrides:\n  react: *version\n";
    let manifest = project(serde_json::json!({ "dependencies": { "other": "1.0.0" } }));
    let output = run_cleanup(Some(original), None, &[&manifest]).unwrap();
    assert_eq!(output, "overrides:\n  react: &version ^1.0.0\n");
}

#[test]
fn deleting_a_setting_preserves_catalog_aliases() {
    let original = "version: &version ^1.0.0\ncatalog:\n  react: *version\n  react-dom: *version\n";
    let output = super::run_update_field(Some(original), "version", &serde_json::Value::Null);
    assert_eq!(
        output,
        Some("catalog:\n  react: &version ^1.0.0\n  react-dom: *version\n".to_string())
    );
}

#[test]
fn repeated_anchor_names_do_not_merge_distinct_groups() {
    let original =
        "catalog:\n  a: &version ^1.0.0\n  b: *version\n  c: &version ^2.0.0\n  d: *version\n";
    let output =
        run(Some(original), &catalogs(&[("default", &[("a", "^3.0.0"), ("b", "^3.0.0")])]))
            .unwrap();
    let value: serde_json::Value = serde_saphyr::from_str(&output).unwrap();
    assert_eq!(
        value["catalog"],
        serde_json::json!({"a": "^3.0.0", "b": "^3.0.0", "c": "^2.0.0", "d": "^2.0.0"})
    );
    assert!(output.contains("b: *version_1"), "{output}");
    assert!(output.contains("d: *version_2"), "{output}");
}

#[test]
fn multiline_and_unicode_scalar_aliases_survive_unrelated_edits() {
    let original = "description: &text |\n  café\n  second line\nextraEnv:\n  DESCRIPTION: *text\ncatalog:\n  react: ^1.0.0\n";
    let output = run(Some(original), &catalogs(&[("default", &[("react", "^2.0.0")])])).unwrap();
    assert_eq!(output, original.replace("^1.0.0", "^2.0.0"));
}

#[test]
fn tagged_scalar_updates_do_not_change_other_entries() {
    let original = "catalog:\n  react: &version !!str 1.0\n  react-dom: *version\n";
    let output = run(Some(original), &catalogs(&[("default", &[("react", "2.0")])])).unwrap();
    let value: serde_json::Value = serde_saphyr::from_str(&output).unwrap();
    assert_eq!(value["catalog"], serde_json::json!({ "react": "2.0", "react-dom": "1.0" }));
}
