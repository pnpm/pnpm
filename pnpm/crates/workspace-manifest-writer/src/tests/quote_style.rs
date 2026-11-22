//! Scalars a write adds take the quote style the manifest already uses.

use serde_json::json;

use super::{
    catalogs,
    run,
    run_allow_builds,
    run_update_field,
};

const EXCLUDE: &str = "minimumReleaseAgeExclude";

#[test]
fn config_set_double_quotes_new_entries_in_a_double_quoted_manifest() {
    let original = "packages:\n  - \".\"\nminimumReleaseAge: 1440\n";
    let out = run_update_field(Some(original), EXCLUDE, &json!(["@scope/b@1.0.0", "foo@1.0.0"]));
    assert_eq!(
        out.as_deref(),
        Some(
            "packages:\n  - \".\"\nminimumReleaseAge: 1440\nminimumReleaseAgeExclude:\n  - \"@scope/b@1.0.0\"\n  - foo@1.0.0\n",
        ),
    );
}

#[test]
fn config_set_keeps_single_quotes_in_a_single_quoted_manifest() {
    let original = "packages:\n  - '.'\nminimumReleaseAge: 1440\n";
    let out = run_update_field(Some(original), EXCLUDE, &json!(["@scope/b@1.0.0"]));
    assert_eq!(
        out.as_deref(),
        Some(
            "packages:\n  - '.'\nminimumReleaseAge: 1440\nminimumReleaseAgeExclude:\n  - '@scope/b@1.0.0'\n",
        ),
    );
}

#[test]
fn config_set_keeps_single_quotes_in_an_unquoted_or_new_manifest() {
    let original = "packages:\n  - .\nminimumReleaseAge: 1440\n";
    let out = run_update_field(Some(original), EXCLUDE, &json!(["@scope/b@1.0.0"]));
    assert_eq!(
        out.as_deref(),
        Some(
            "packages:\n  - .\nminimumReleaseAge: 1440\nminimumReleaseAgeExclude:\n  - '@scope/b@1.0.0'\n",
        ),
    );

    let out = run_update_field(None, EXCLUDE, &json!(["@scope/b@1.0.0"]));
    assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude:\n  - '@scope/b@1.0.0'\n"));
}

#[test]
fn allow_builds_double_quotes_new_keys_in_a_double_quoted_manifest() {
    let original = "packages:\n  - \"packages/*\"\n";
    let out = run_allow_builds(Some(original), &[("@scope/native", true), ("esbuild", true)]);
    assert_eq!(
        out.as_deref(),
        Some(
            "packages:\n  - \"packages/*\"\nallowBuilds:\n  \"@scope/native\": true\n  esbuild: true\n",
        ),
    );
}

#[test]
fn allow_builds_keeps_single_quotes_in_a_single_quoted_manifest() {
    let original = "packages:\n  - 'packages/*'\n";
    let out = run_allow_builds(Some(original), &[("@scope/native", true)]);
    assert_eq!(
        out.as_deref(),
        Some("packages:\n  - 'packages/*'\nallowBuilds:\n  '@scope/native': true\n"),
    );
}

#[test]
fn existing_single_quoted_scalars_stay_as_written_in_a_double_quoted_manifest() {
    let original =
        "packages:\n  - \"apps/*\"\n  - \"packages/*\"\nallowBuilds:\n  '@scope/kept': true\n";
    let out = run_allow_builds(Some(original), &[("@scope/added", false)]);
    assert_eq!(
        out.as_deref(),
        Some(
            "packages:\n  - \"apps/*\"\n  - \"packages/*\"\nallowBuilds:\n  \"@scope/added\": false\n  '@scope/kept': true\n",
        ),
    );
}

#[test]
fn catalog_entries_double_quote_new_keys_in_a_double_quoted_manifest() {
    let original = "packages:\n  - \"packages/*\"\ncatalog:\n  react: \"^18.0.0\"\n";
    let out = run(Some(original), &catalogs(&[("default", &[("@types/react", "^18.0.0")])]));
    assert_eq!(
        out.as_deref(),
        Some(
            "packages:\n  - \"packages/*\"\ncatalog:\n  \"@types/react\": ^18.0.0\n  react: \"^18.0.0\"\n",
        ),
    );
}
