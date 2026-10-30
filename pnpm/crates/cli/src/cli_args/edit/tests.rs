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
    assert!(parse_package_path("express/").is_err());
    assert!(parse_package_path("/express").is_err());
    assert!(parse_package_path("foo:bar").is_err());
    assert!(parse_package_path("foo/./bar").is_err());
    assert!(parse_package_path("@scope/").is_err());
    assert!(parse_package_path(r"foo\..\bar").is_err());
}

#[test]
fn editor_arguments_preserve_empty_values_and_reject_unclosed_quotes() {
    let root = tempdir().unwrap();
    let command =
        super::resolve_editor_command("/editor --wait ''", root.path(), root.path()).unwrap();
    let arguments: Vec<_> = command.get_args().collect();
    assert_eq!(
        arguments,
        vec![std::ffi::OsStr::new("--wait"), std::ffi::OsStr::new(""), root.path().as_os_str()],
    );
    let error =
        super::resolve_editor_command("/editor 'unfinished", root.path(), root.path()).unwrap_err();
    assert!(error.to_string().contains("Invalid editor command"), "{error}");
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

#[cfg(unix)]
#[test]
fn de_hardlink_preserves_executable_mode_and_does_not_follow_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempdir().unwrap();
    let package = root.path().join("package");
    fs::create_dir(&package).unwrap();
    let store_file = root.path().join("store-file");
    fs::write(&store_file, "original").unwrap();
    fs::set_permissions(&store_file, fs::Permissions::from_mode(0o555)).unwrap();
    let target = package.join("executable");
    fs::hard_link(&store_file, &target).unwrap();
    symlink(&store_file, package.join("symlink")).unwrap();
    de_hardlink_dir(&package).unwrap();
    assert_eq!(
        fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755,
    );
    assert_eq!(
        fs::metadata(&store_file)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o555,
    );
    assert!(
        fs::symlink_metadata(package.join("symlink"))
            .unwrap()
            .file_type()
            .is_symlink(),
    );
    fs::write(&target, "edited").unwrap();
    assert_eq!(fs::read_to_string(&store_file).unwrap(), "original");
}

#[cfg(unix)]
#[test]
fn package_resolution_rejects_external_links_and_accepts_external_virtual_store() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    let directory = root.path().join("directory");
    let alias = root.path().join("alias");
    fs::create_dir(&directory).unwrap();
    symlink(&directory, &alias).unwrap();
    let modules = alias.join("node_modules");
    let external = alias.join("external");
    let store = alias.join("virtual-store");
    fs::create_dir(&modules).unwrap();
    fs::create_dir(&external).unwrap();
    fs::create_dir(&store).unwrap();
    symlink(&external, modules.join("unsafe")).unwrap();
    let error = super::resolve_package_dir(&modules, &["unsafe".to_owned()], &store).unwrap_err();
    assert!(error.to_string().contains("outside the expected"), "{error}");
    let package = store.join("package");
    fs::create_dir_all(package.join("node_modules/@scope/child")).unwrap();
    symlink(&package, modules.join("safe")).unwrap();
    assert_eq!(
        super::resolve_package_dir(
            &modules,
            &["safe".to_owned(), "@scope/child".to_owned()],
            &store
        )
        .unwrap(),
        dunce::canonicalize(package.join("node_modules/@scope/child")).unwrap(),
    );
}
