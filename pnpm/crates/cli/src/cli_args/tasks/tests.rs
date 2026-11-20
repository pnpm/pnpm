use super::group_dir;
use std::path::Path;

#[test]
fn group_dir_rejects_parent_segments() {
    assert!(group_dir(Path::new("/slots"), "..").is_err());
    assert!(group_dir(Path::new("/slots"), "a/b").is_err());
    assert!(group_dir(Path::new("/slots"), "cargo").expect("plain name").ends_with("cargo"));
}
