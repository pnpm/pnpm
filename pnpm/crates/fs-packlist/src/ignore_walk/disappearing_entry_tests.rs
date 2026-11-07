use super::{DirDefaults, DirRules, Walker};
use std::{fs, io};

#[test]
fn disappearing_listed_directory_is_skipped_but_missing_root_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    let temporary = root.path().join("temporary");
    fs::create_dir(&temporary).unwrap();
    let entry = fs::read_dir(root.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    fs::remove_dir(&temporary).unwrap();
    let rules = DirRules {
        parent: None,
        name: String::new(),
        named_only: false,
        rel_path: String::new(),
        has_exclusions: false,
        defaults: DirDefaults::root(),
        rules: Vec::new(),
        exact: false,
    };
    let mut walker = Walker {
        named_files: None,
        on_file: &mut |_, _| panic!("a removed directory has no files"),
    };
    walker.visit_listed_entry(&entry, &rules, 0).unwrap();
    let error = walker.walk_dir(&temporary, &rules, 0).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}
