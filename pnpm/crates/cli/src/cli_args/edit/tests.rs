use super::{de_hardlink_dir, parse_package_path, split_shell_args};
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
    assert!(parse_package_path("express/").is_err());
    assert!(parse_package_path("/express").is_err());
    assert!(parse_package_path("foo:bar").is_err());
    assert!(parse_package_path("foo/./bar").is_err());
    assert!(parse_package_path("@scope/").is_err());
    assert!(parse_package_path("foo\\..\\bar").is_err());
}

#[test]
fn test_split_shell_args() {
    assert_eq!(split_shell_args("vi"), vec!["vi"]);
    assert_eq!(split_shell_args("code --wait"), vec!["code", "--wait"]);
    assert_eq!(split_shell_args(""), Vec::<String>::new());
    assert_eq!(split_shell_args("   "), Vec::<String>::new());
    assert_eq!(
        split_shell_args("node -e \"process.exit(0)\""),
        vec!["node", "-e", "process.exit(0)"],
    );
    assert_eq!(
        split_shell_args("node -e 'process.exit(0)'"),
        vec!["node", "-e", "process.exit(0)"],
    );
    assert_eq!(split_shell_args("my\\ editor"), vec!["my editor"]);
}

#[test]
fn test_de_hardlink_dir() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("index.js");
    fs::write(&file_path, "original").unwrap();

    let link_path = tmp.path().join("index-link.js");
    fs::hard_link(&file_path, &link_path).unwrap();

    #[cfg(unix)]
    {
        let meta_orig = fs::metadata(&file_path).unwrap();
        let meta_link = fs::metadata(&link_path).unwrap();

        use std::os::unix::fs::MetadataExt;
        assert_eq!(meta_orig.ino(), meta_link.ino());
    }

    de_hardlink_dir(tmp.path()).unwrap();

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "original");

    #[cfg(unix)]
    {
        let meta_orig_after = fs::metadata(&file_path).unwrap();
        let meta_link_after = fs::metadata(&link_path).unwrap();

        use std::os::unix::fs::MetadataExt;
        assert_ne!(meta_orig_after.ino(), meta_link_after.ino());
    }

    fs::write(&file_path, "modified").unwrap();
    assert_eq!(fs::read_to_string(&file_path).unwrap(), "modified");
    assert_eq!(fs::read_to_string(&link_path).unwrap(), "original");
}
