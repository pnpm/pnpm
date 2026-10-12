use super::hook_path;
use std::{env, ffi::OsString, path::PathBuf};

fn entries(path: &OsString) -> Vec<PathBuf> {
    env::split_paths(path).collect()
}

#[test]
fn hook_path_is_the_scripts_directory_alone_when_path_is_unset() {
    let path = hook_path(PathBuf::from("scripts"), None).unwrap();
    assert_eq!(entries(&path), [PathBuf::from("scripts")]);
}

#[test]
fn hook_path_puts_the_scripts_directory_before_the_inherited_entries() {
    let inherited = env::join_paths(["first", "second"]).unwrap();
    let path = hook_path(PathBuf::from("scripts"), Some(&inherited)).unwrap();
    assert_eq!(
        entries(&path),
        [PathBuf::from("scripts"), PathBuf::from("first"), PathBuf::from("second")],
    );
}

#[test]
fn hook_path_drops_empty_inherited_entries() {
    for inherited in [OsString::new(), env::join_paths(["", "first", "", "second", ""]).unwrap()] {
        let path = hook_path(PathBuf::from("scripts"), Some(&inherited)).unwrap();
        assert!(
            entries(&path)
                .iter()
                .all(|entry| !entry.as_os_str().is_empty()),
            "{inherited:?} became {path:?}",
        );
    }
}
