use super::importer_id;
use std::path::Path;

#[test]
fn importer_id_supports_a_lockfile_directory_below_the_project() {
    let project = tempfile::tempdir().unwrap();
    let lockfile_dir = project.path().join("locks");
    assert_eq!(importer_id(project.path(), Path::new(&lockfile_dir)), "..");
}
