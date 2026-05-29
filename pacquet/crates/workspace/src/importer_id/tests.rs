use super::importer_id_from_root_dir;
use std::path::Path;

#[test]
fn returns_dot_for_root() {
    assert_eq!(importer_id_from_root_dir(Path::new("/ws"), Path::new("/ws")), ".");
}

#[test]
fn returns_posix_relative_for_subproject() {
    assert_eq!(
        importer_id_from_root_dir(Path::new("/ws"), Path::new("/ws/packages/a")),
        "packages/a",
    );
}

#[test]
fn nested_subproject() {
    assert_eq!(importer_id_from_root_dir(Path::new("/ws"), Path::new("/ws/a/b/c")), "a/b/c");
}
