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
    let out = run_update_field(Some(original), EXCLUDE, &json!(["@scope/b@1.0.0", "foo@1.0.0"]))
        .expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(
        out,
        "packages:\n  - \".\"\nminimumReleaseAge: 1440\nminimumReleaseAgeExclude:\n  - \"@scope/b@1.0.0\"\n  - foo@1.0.0\n",
    );
}

#[test]
fn config_set_keeps_single_quotes_in_a_single_quoted_manifest() {
    let original = "packages:\n  - '.'\nminimumReleaseAge: 1440\n";
    let out = run_update_field(Some(original), EXCLUDE, &json!(["@scope/b@1.0.0"]))
        .expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(
        out,
        "packages:\n  - '.'\nminimumReleaseAge: 1440\nminimumReleaseAgeExclude:\n  - '@scope/b@1.0.0'\n",
    );
}

#[test]
fn config_set_keeps_single_quotes_in_an_unquoted_or_new_manifest() {
    let original = "packages:\n  - .\nminimumReleaseAge: 1440\n";
    let out = run_update_field(Some(original), EXCLUDE, &json!(["@scope/b@1.0.0"]))
        .expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(
        out,
        "packages:\n  - .\nminimumReleaseAge: 1440\nminimumReleaseAgeExclude:\n  - '@scope/b@1.0.0'\n",
    );

    let out = run_update_field(None, EXCLUDE, &json!(["@scope/b@1.0.0"])).expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(out, "minimumReleaseAgeExclude:\n  - '@scope/b@1.0.0'\n");
}

#[test]
fn allow_builds_double_quotes_new_keys_in_a_double_quoted_manifest() {
    let original = "packages:\n  - \"packages/*\"\n";
    let out = run_allow_builds(Some(original), &[("@scope/native", true), ("esbuild", true)])
        .expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(
        out,
        "packages:\n  - \"packages/*\"\nallowBuilds:\n  \"@scope/native\": true\n  esbuild: true\n",
    );
}

#[test]
fn allow_builds_keeps_single_quotes_in_a_single_quoted_manifest() {
    let original = "packages:\n  - 'packages/*'\n";
    let out = run_allow_builds(Some(original), &[("@scope/native", true)]).expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(out, "packages:\n  - 'packages/*'\nallowBuilds:\n  '@scope/native': true\n");
}

#[test]
fn existing_single_quoted_scalars_stay_as_written_in_a_double_quoted_manifest() {
    let original =
        "packages:\n  - \"apps/*\"\n  - \"packages/*\"\nallowBuilds:\n  '@scope/kept': true\n";
    let out = run_allow_builds(Some(original), &[("@scope/added", false)]).expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(
        out,
        "packages:\n  - \"apps/*\"\n  - \"packages/*\"\nallowBuilds:\n  \"@scope/added\": false\n  '@scope/kept': true\n",
    );
}

#[test]
fn an_added_scalar_that_sorts_before_an_equal_existing_one_takes_the_double_quotes() {
    let original =
        "packages:\n  - \"apps/*\"\n  - \"packages/*\"\ncatalog:\n  '@scope/x': ^1.0.0\n";
    let out = run_allow_builds(Some(original), &[("@scope/x", true)]).expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(
        out,
        "packages:\n  - \"apps/*\"\n  - \"packages/*\"\nallowBuilds:\n  \"@scope/x\": true\ncatalog:\n  '@scope/x': ^1.0.0\n",
    );
}

#[test]
fn catalog_entries_double_quote_new_keys_in_a_double_quoted_manifest() {
    let original = "packages:\n  - \"packages/*\"\ncatalog:\n  react: \"^18.0.0\"\n";
    let out = run(Some(original), &catalogs(&[("default", &[("@types/react", "^18.0.0")])]))
        .expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(
        out,
        "packages:\n  - \"packages/*\"\ncatalog:\n  \"@types/react\": ^18.0.0\n  react: \"^18.0.0\"\n",
    );
}

#[test]
fn config_set_double_quotes_a_value_moved_to_another_key() {
    let original = "packages:\n  - \"apps/*\"\n  - \"packages/*\"\noverrides:\n  a: '>=1.0.0'\n  b: 1.0.0\n  c: 2.0.0\n  z: 3.0.0\n";
    let overrides = json!({ "a": "3.0.0", "b": "1.0.0", "c": "2.0.0", "z": ">=1.0.0" });
    let out = run_update_field(Some(original), "overrides", &overrides).expect("file written");
    eprintln!("OUTPUT:\n{out}\n");
    assert_eq!(
        out,
        "packages:\n  - \"apps/*\"\n  - \"packages/*\"\noverrides:\n  a: 3.0.0\n  b: 1.0.0\n  c: 2.0.0\n  z: \">=1.0.0\"\n",
    );
}
