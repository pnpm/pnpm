use super::{de_hardlink_dir, parse_package_path};
use std::fs;
use tempfile::tempdir;

#[test]
fn test_parse_package_path() {
    assert_eq!(parse_package_path("express").unwrap(), vec!["express"]);
    assert_eq!(parse_package_path("@types/node").unwrap(), vec!["@types/node"]);
    assert_eq!(parse_package_path("express/safe-buffer").unwrap(), vec!["express", "safe-buffer",]);
    assert_eq!(parse_package_path("@scope/foo/bar").unwrap(), vec!["@scope/foo", "bar"]);
    assert!(parse_package_path("").is_err());
    assert!(parse_package_path("..").is_err());
    assert!(parse_package_path("foo/../bar").is_err());
    assert!(parse_package_path("@scope").is_err());
}

#[test]
fn test_de_hardlink_dir() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("index.js");
    fs::write(&file_path, "original").unwrap();

    let link_path = tmp.path().join("index-link.js");
    fs::hard_link(&file_path, &link_path).unwrap();

    // Verify they share the same inode initially on unix
    let meta_orig = fs::metadata(&file_path).unwrap();
    let meta_link = fs::metadata(&link_path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(meta_orig.ino(), meta_link.ino());
    }

    // Run de-hardlink
    de_hardlink_dir(tmp.path()).unwrap();

    // Verify the edited path has the original content
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "original");

    // Verify the files no longer share the same inode / hard link on unix
    let meta_orig_after = fs::metadata(&file_path).unwrap();
    let meta_link_after = fs::metadata(&link_path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_ne!(meta_orig_after.ino(), meta_link_after.ino());
    }

    // Verify editing one does not affect the other
    fs::write(&file_path, "modified").unwrap();
    assert_eq!(fs::read_to_string(&file_path).unwrap(), "modified");
    assert_eq!(fs::read_to_string(&link_path).unwrap(), "original");
}
