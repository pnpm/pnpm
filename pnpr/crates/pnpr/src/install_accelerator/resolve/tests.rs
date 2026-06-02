use super::{importer_manifest_name, sanitized_importer_dir};

#[test]
fn root_and_trailing_slashes_normalize_to_dot() {
    assert_eq!(sanitized_importer_dir(".").unwrap(), ".");
    assert_eq!(sanitized_importer_dir("").unwrap(), ".");
    assert_eq!(sanitized_importer_dir("packages/foo/").unwrap(), "packages/foo");
}

#[test]
fn nested_member_dirs_pass_through() {
    assert_eq!(sanitized_importer_dir("project-a").unwrap(), "project-a");
    assert_eq!(sanitized_importer_dir("packages/foo").unwrap(), "packages/foo");
}

#[test]
fn traversal_absolute_and_backslash_dirs_are_rejected() {
    // `/` and `////` are slashes-only: they must be rejected, not trimmed
    // down to the root importer.
    for unsafe_dir in
        ["../escape", "packages/../../etc", "/abs/path", r"packages\foo", "a//b", "/", "////"]
    {
        assert!(
            sanitized_importer_dir(unsafe_dir).is_err(),
            "expected {unsafe_dir:?} to be rejected",
        );
    }
}

#[test]
fn manifest_names_are_distinct_per_dir() {
    assert_eq!(importer_manifest_name("."), "pnpr-resolve");
    assert_ne!(importer_manifest_name("packages/foo"), importer_manifest_name("packages/bar"));
    // `/` → `-` alone would collide these two; escaping `-` first keeps
    // the mapping injective.
    assert_ne!(importer_manifest_name("packages/foo"), importer_manifest_name("packages-foo"));
}
