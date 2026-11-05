use super::write_record;
use std::fs;

#[test]
fn a_record_hard_linked_to_a_store_file_leaves_the_store_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let store_file = dir.path().join("store-blob");
    fs::write(&store_file, "shipped by the package").unwrap();
    let record = dir.path().join(".pnpm-linked-archive");
    fs::hard_link(&store_file, &record).unwrap();

    write_record(&record, "sha512-linked");

    assert_eq!(fs::read_to_string(&record).unwrap(), "sha512-linked");
    assert_eq!(fs::read_to_string(&store_file).unwrap(), "shipped by the package");
}
