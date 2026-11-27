use super::{
    EntryPathRejection,
    archive_entry_segments,
    clean_archive_entry_path,
    has_windows_drive_prefix,
    normalize_bin_path,
    normalize_package_relative_path,
};
use pretty_assertions::assert_eq;

#[test]
fn splits_entry_paths_on_both_separators() {
    assert_eq!(
        archive_entry_segments(r"package\lib/index.js"),
        Some(vec!["package", "lib", "index.js"]),
    );
    assert_eq!(archive_entry_segments("./package/a"), Some(vec![".", "package", "a"]));
    assert_eq!(archive_entry_segments("package/./a//b"), Some(vec!["package", "a", "b"]));
}

#[test]
fn rejects_entry_paths_that_leave_the_archive() {
    for raw in ["/etc/passwd", r"\x", "package/../../x", "package/C:/x", "", "//"] {
        assert_eq!(archive_entry_segments(raw), None, "{raw:?}");
    }
}

#[test]
fn cleans_entry_paths() {
    assert_eq!(
        clean_archive_entry_path("package/lib/index.js").ok(),
        Some("lib/index.js".to_string()),
    );
    assert_eq!(clean_archive_entry_path("index.js").ok(), Some("index.js".to_string()));
    assert_eq!(clean_archive_entry_path("./package/a").ok(), Some("package/a".to_string()));
    assert!(matches!(clean_archive_entry_path("."), Err(EntryPathRejection::ArchiveRoot)));
    assert!(matches!(
        clean_archive_entry_path("../x"),
        Err(EntryPathRejection::NonNormalComponent)
    ));
}

#[test]
fn normalizes_package_relative_paths() {
    assert_eq!(normalize_package_relative_path("bin/./cli.js"), Some("bin/cli.js".to_string()));
    assert_eq!(normalize_package_relative_path("a/b/../c"), Some("a/c".to_string()));
    for raw in ["/bin", r"a\b", "c:x", "../x"] {
        assert_eq!(normalize_package_relative_path(raw), None, "{raw:?}");
    }
}

#[test]
fn normalizes_bin_paths() {
    assert_eq!(normalize_bin_path(r"bin\cli.js"), Some("bin/cli.js".to_string()));
    assert_eq!(normalize_bin_path("../../cli.js"), Some("cli.js".to_string()));
    for raw in ["C:/cli.js", "/cli.js", r"\cli.js", "x/../C:/cli.js"] {
        assert_eq!(normalize_bin_path(raw), None, "{raw:?}");
    }
}

#[test]
fn recognizes_windows_drives() {
    assert!(has_windows_drive_prefix("C:"));
    assert!(has_windows_drive_prefix("z:/x"));
    for path in ["", "C", "1:", "é:", ":C", "CC:"] {
        assert!(!has_windows_drive_prefix(path), "{path:?}");
    }
}
