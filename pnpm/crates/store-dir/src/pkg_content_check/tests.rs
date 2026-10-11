use super::pkg_content_mismatch;
use crate::store_index_key;

fn manifest(name: &str, version: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "version": version })
}

fn key(pkg_id: &str) -> String {
    store_index_key("sha512-abc", pkg_id)
}

#[test]
fn matching_name_and_version_is_no_mismatch() {
    assert_eq!(pkg_content_mismatch(Some(&manifest("foo", "1.0.0")), &key("foo@1.0.0")), None);
}

#[test]
fn scoped_names_split_at_the_version_separator() {
    assert_eq!(
        pkg_content_mismatch(Some(&manifest("@scope/foo", "1.0.0")), &key("@scope/foo@1.0.0")),
        None,
    );
}

#[test]
fn a_different_name_mismatches() {
    let mismatch = pkg_content_mismatch(Some(&manifest("bar", "1.0.0")), &key("foo@1.0.0"))
        .expect("a row holding another package must be reported");
    assert_eq!(mismatch.expected, "foo@1.0.0");
    assert_eq!(mismatch.actual, "bar@1.0.0");
}

#[test]
fn a_different_version_mismatches() {
    let mismatch = pkg_content_mismatch(Some(&manifest("foo", "2.0.0")), &key("foo@1.0.0"))
        .expect("a row holding another version must be reported");
    assert_eq!(mismatch.expected, "foo@1.0.0");
    assert_eq!(mismatch.actual, "foo@2.0.0");
}

#[test]
fn names_are_compared_case_insensitively() {
    assert_eq!(pkg_content_mismatch(Some(&manifest("Foo", "1.0.0")), &key("foo@1.0.0")), None);
}

#[test]
fn versions_are_compared_as_semver() {
    assert_eq!(pkg_content_mismatch(Some(&manifest("foo", "v1.0.0")), &key("foo@1.0.0")), None);
}

/// A field the manifest does not state cannot disagree with the key,
/// but the field next to it still can — and the message renders the
/// absent half the way pnpm does.
#[test]
fn an_absent_manifest_field_is_not_compared() {
    assert_eq!(
        pkg_content_mismatch(Some(&serde_json::json!({ "name": "foo" })), &key("foo@1.0.0")),
        None,
    );
    let mismatch =
        pkg_content_mismatch(Some(&serde_json::json!({ "name": "bar" })), &key("foo@1.0.0"))
            .expect("the name still disagrees");
    assert_eq!(mismatch.actual, "bar@undefined");
}

/// Rows pnpm wrote before it kept manifests, and rows for packages
/// whose `package.json` failed to parse, carry no identity to check.
#[test]
fn a_row_without_a_manifest_is_not_checked() {
    assert_eq!(pkg_content_mismatch(None, &key("foo@1.0.0")), None);
}

/// A package resolved from a named registry is keyed
/// `<name>@<alias>:<version>`. The alias says where the version came
/// from, not which version it is, so the manifest is compared against
/// the version behind it.
#[test]
fn a_registry_qualified_key_is_compared_against_the_version_it_qualifies() {
    assert_eq!(pkg_content_mismatch(Some(&manifest("foo", "1.0.0")), &key("foo@work:1.0.0")), None);
    let mismatch = pkg_content_mismatch(Some(&manifest("bar", "1.0.0")), &key("foo@work:1.0.0"))
        .expect("a row holding another package must be reported");
    assert_eq!(mismatch.expected, "foo@1.0.0");
}

/// Only registry resolutions are keyed by `name@version`. A tarball URL
/// has an `@` without being one — and its name half would be the URL
/// itself, so reporting it would put any credentials the URL carries in
/// the error. Git-hosted rows are keyed `<pkg_id>\t{built,not-built}`,
/// and a version slot that is a `file:` / `link:` reference states no
/// version to disagree with.
#[test]
fn keys_that_name_no_package_are_not_checked() {
    for pkg_id in [
        "https://example.com/foo-1.0.0.tgz",
        "https://user@example.com/foo-1.0.0.tgz",
        "https://user:token@example.com/foo@1.2.3",
        "https://example.com/foo@1.2.3",
        "@scope/foo@not-a-version",
        "foo@file:../bar",
        "foo@link:../bar",
    ] {
        assert_eq!(
            pkg_content_mismatch(Some(&manifest("bar", "2.0.0")), &key(pkg_id)),
            None,
            "{pkg_id} names no package to check against",
        );
    }
    assert_eq!(
        pkg_content_mismatch(
            Some(&manifest("bar", "2.0.0")),
            &crate::git_hosted_store_index_key("github.com/foo/bar#deadbeef", true),
        ),
        None,
    );
}
