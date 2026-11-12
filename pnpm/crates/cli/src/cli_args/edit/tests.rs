use super::{
    de_hardlink_dir, package_rebuild_selection, parse_package_path, resolve_package_dir,
    split_shell_args,
};
use std::{fs, path::Path};
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
fn test_split_shell_args() {
    assert_eq!(split_shell_args("vi"), vec!["vi"]);
    assert_eq!(split_shell_args("code --wait"), vec!["code", "--wait"]);
    assert_eq!(split_shell_args(""), Vec::<String>::new());
    assert_eq!(split_shell_args("   "), Vec::<String>::new());
    assert_eq!(
        split_shell_args(r#"node -e "process.exit(0)""#),
        vec!["node", "-e", "process.exit(0)"],
    );
    assert_eq!(
        split_shell_args("node -e 'process.exit(0)'"),
        vec!["node", "-e", "process.exit(0)"],
    );
    assert_eq!(split_shell_args(r"my\ editor"), vec!["my editor"]);
    assert_eq!(split_shell_args(r#"emacsclient -a """#), vec!["emacsclient", "-a", ""]);
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

#[test]
fn rebuild_selection_uses_registry_version_and_source_package_name() {
    let tmp = tempdir().unwrap();
    let registry_package = tmp.path().join("foo@1.2.3/node_modules/foo");
    let source_package = tmp.path().join("foo@git+https___example.com_repo/node_modules/foo");
    fs::create_dir_all(&registry_package).unwrap();
    fs::create_dir_all(&source_package).unwrap();
    for package_dir in [&registry_package, &source_package] {
        fs::write(package_dir.join("package.json"), r#"{"name":"foo","version":"1.2.3"}"#).unwrap();
    }

    assert_eq!(package_rebuild_selection(&registry_package, 120).unwrap(), ["foo@1.2.3"]);
    assert_eq!(package_rebuild_selection(&source_package, 120).unwrap(), ["foo"]);
}

#[cfg(unix)]
#[test]
fn resolves_a_nested_dependency_from_the_virtual_store_node_modules() {
    let tmp = tempdir().unwrap();
    let modules_dir = tmp.path().join("node_modules");
    let package_node_modules = tmp.path().join("node_modules/.pnpm/parent@1.0.0/node_modules");
    let parent_dir = package_node_modules.join("parent");
    let child_dir = package_node_modules.join("child");
    fs::create_dir_all(&parent_dir).unwrap();
    fs::create_dir_all(&child_dir).unwrap();
    std::os::unix::fs::symlink(&parent_dir, modules_dir.join("parent")).unwrap();

    let resolved = resolve_package_dir(
        &modules_dir,
        &["parent".to_owned(), "child".to_owned()],
        Path::new("/unused"),
    )
    .unwrap();

    assert_eq!(resolved, dunce::canonicalize(child_dir).unwrap());
}

#[cfg(unix)]
#[test]
fn resolves_a_nested_dependency_from_the_package_before_a_hoisted_copy() {
    let tmp = tempdir().unwrap();
    let modules_dir = tmp.path().join("node_modules");
    let package_node_modules = tmp.path().join("node_modules/.pnpm/parent@1.0.0/node_modules");
    let parent_dir = package_node_modules.join("parent");
    let nested_dir = parent_dir.join("node_modules/child");
    let hoisted_dir = package_node_modules.join("child");
    fs::create_dir_all(&nested_dir).unwrap();
    fs::create_dir_all(&hoisted_dir).unwrap();
    std::os::unix::fs::symlink(&parent_dir, modules_dir.join("parent")).unwrap();

    let resolved = resolve_package_dir(
        &modules_dir,
        &["parent".to_owned(), "child".to_owned()],
        Path::new("/unused"),
    )
    .unwrap();

    assert_eq!(resolved, dunce::canonicalize(nested_dir).unwrap());
}

#[cfg(unix)]
#[test]
fn resolves_a_nested_dependency_from_a_scoped_package() {
    let tmp = tempdir().unwrap();
    let modules_dir = tmp.path().join("node_modules");
    let package_node_modules = tmp.path().join("node_modules/.pnpm/parent@1.0.0/node_modules");
    let parent_dir = package_node_modules.join("@scope/parent");
    let child_dir = package_node_modules.join("child");
    fs::create_dir_all(&parent_dir).unwrap();
    fs::create_dir_all(&child_dir).unwrap();
    fs::create_dir_all(modules_dir.join("@scope")).unwrap();
    std::os::unix::fs::symlink(&parent_dir, modules_dir.join("@scope/parent")).unwrap();

    let resolved = resolve_package_dir(
        &modules_dir,
        &["@scope/parent".to_owned(), "child".to_owned()],
        Path::new("/unused"),
    )
    .unwrap();

    assert_eq!(resolved, dunce::canonicalize(child_dir).unwrap());
}
