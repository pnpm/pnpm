use super::{
    TempDir, UpdateWorkspaceManifestOptions, WORKSPACE_MANIFEST_FILENAME, catalogs, fs, run,
    update_workspace_manifest,
};

#[test]
fn empty_catalogs_does_not_create_a_file() {
    assert_eq!(run(None, &catalogs(&[])), None);
    assert_eq!(run(None, &catalogs(&[("default", &[])])), None);
    assert_eq!(run(None, &catalogs(&[("foo", &[]), ("bar", &[])])), None);
}

#[test]
fn default_catalog_goes_to_the_catalog_shorthand() {
    let out = run(None, &catalogs(&[("default", &[("foo", "^0.1.2")])])).expect("file written");
    assert_eq!(out, "catalog:\n  foo: ^0.1.2\n");
}

#[test]
fn default_merges_into_existing_catalog_shorthand() {
    let original = "catalog:\n  bar: 3.2.1\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("foo", "^0.1.2")])])).expect("written");
    assert_eq!(out, "catalog:\n  bar: 3.2.1\n  foo: ^0.1.2\n");
}

#[test]
fn default_merges_into_existing_catalogs_default() {
    let original = "catalogs:\n  default:\n    bar: 3.2.1\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("foo", "^0.1.2")])])).expect("written");
    assert_eq!(out, "catalogs:\n  default:\n    bar: 3.2.1\n    foo: ^0.1.2\n");
}

#[test]
fn named_catalogs_create_a_catalogs_block() {
    let out = run(None, &catalogs(&[("bar", &[("def", "3.2.1")]), ("foo", &[("abc", "0.1.2")])]))
        .expect("written");
    assert_eq!(out, "catalogs:\n  bar:\n    def: 3.2.1\n  foo:\n    abc: 0.1.2\n");
}

#[test]
fn named_catalog_added_to_existing_catalogs() {
    let original = "catalogs:\n  foo:\n    ghi: 7.8.9\n";
    let out = run(
        Some(original),
        &catalogs(&[("bar", &[("def", "3.2.1")]), ("foo", &[("abc", "0.1.2")])]),
    )
    .expect("written");
    assert_eq!(out, "catalogs:\n  bar:\n    def: 3.2.1\n  foo:\n    abc: 0.1.2\n    ghi: 7.8.9\n");
}

#[test]
fn adds_a_new_catalog_after_packages() {
    let original = "packages:\n  - '*'\n";
    let out = run(Some(original), &catalogs(&[("default", &[("foo", "1.0.0")])])).expect("written");
    assert_eq!(out, "packages:\n  - '*'\ncatalog:\n  foo: 1.0.0\n");
}

#[test]
fn preserves_blank_lines_when_inserting_a_catalog_between_fields() {
    let original =
        "packages:\n  - '*'\n\nallowBuilds:\n  foo: true\n\noverrides:\n  foo: '1.0.0'\n";
    let out = run(Some(original), &catalogs(&[("default", &[("bar", "2.0.0")])])).expect("written");
    assert_eq!(
        out,
        "packages:\n  - '*'\n\nallowBuilds:\n  foo: true\n\ncatalog:\n  bar: 2.0.0\n\noverrides:\n  foo: '1.0.0'\n",
    );
}

#[test]
fn catalog_sorts_to_front_with_blank_line_style() {
    let original = "overrides:\n  foo: '2.0.0'\n\npackages:\n  - '*'\n";
    let out = run(Some(original), &catalogs(&[("default", &[("bar", "1.0.0")])])).expect("written");
    assert_eq!(out, "catalog:\n  bar: 1.0.0\n\noverrides:\n  foo: '2.0.0'\n\npackages:\n  - '*'\n");
}

#[test]
fn updates_named_catalog_value_preserving_comment() {
    let original = "catalogs:\n  react:\n    # pinned by the platform team\n    react: 18.0.0\n";
    let out =
        run(Some(original), &catalogs(&[("react", &[("react", "18.2.0")])])).expect("written");
    assert_eq!(out, "catalogs:\n  react:\n    # pinned by the platform team\n    react: 18.2.0\n");
}

/// `saveCatalogName` is unconstrained — it comes from
/// `pnpm-workspace.yaml`, `PNPM_CONFIG_SAVE_CATALOG_NAME`, or
/// `--save-catalog-name`. A newline in it renders as a YAML block scalar
/// that the splice would write into the middle of the `catalogs:`
/// header; U+2028 / U+2029 are subtler, folding the scalar so the name
/// parses back with the folding indentation embedded in it.
#[test]
fn add_catalogs_rejects_control_characters() {
    let original = "packages:\n  - pkgs/*\n";
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, original).expect("seed manifest");

    for updated in [
        catalogs(&[("shared\n  injected: oops", &[("foo", "^1.0.0")])]),
        catalogs(&[("shared", &[("foo\nbar", "^1.0.0")])]),
        catalogs(&[("shared", &[("foo", "^1.0.0\nbaz: qux")])]),
        catalogs(&[("sha\u{2028}red", &[("foo", "^1.0.0")])]),
        catalogs(&[("sha\u{2029}red", &[("foo", "^1.0.0")])]),
    ] {
        let err = update_workspace_manifest(
            dir.path(),
            &UpdateWorkspaceManifestOptions {
                updated_catalogs: Some(&updated),
                ..Default::default()
            },
        )
        .expect_err("must reject a line-break character");

        assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
        assert_eq!(fs::read_to_string(&path).expect("manifest kept"), original);
    }
}
