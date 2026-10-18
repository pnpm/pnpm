use super::super::{StoreIndex, StoreIndexFormat, tests::sample_index};
use pretty_assertions::assert_eq;
use std::{collections::HashSet, path::Path};
use tempfile::tempdir;

const TGZ_KEY: &str = "sha512-tgz\tacme@1.0.0";
const TAR_KEY: &str = "sha512-tar\tacme@1.0.0";

fn assert_alias_reads_its_target(root: &Path) {
    let mut index = StoreIndex::open(root).unwrap();
    let mut row = sample_index();
    row.tar_integrity = Some("sha512-tar".to_string());
    index.set(TGZ_KEY, &row).unwrap();
    index
        .set_aliases(&[(TAR_KEY.to_string(), TGZ_KEY.to_string())])
        .unwrap();

    assert_eq!(index.get(TAR_KEY).unwrap(), Some(row));
    assert!(index.contains_key(TAR_KEY).unwrap());
    let keys =
        vec![TAR_KEY.to_string(), TGZ_KEY.to_string(), "sha512-other\tacme@1.0.0".to_string()];
    let found: HashSet<String> = index
        .get_many_raw(&keys)
        .unwrap()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    assert_eq!(found, keys[..2].iter().cloned().collect());
    assert_eq!(index.contains_many(&keys).unwrap(), keys[..2].iter().cloned().collect());
}

#[test]
fn an_sqlite_alias_reads_its_target_row() {
    let dir = tempdir().unwrap();
    assert_alias_reads_its_target(dir.path());
}

#[test]
fn a_files_alias_reads_its_target_row() {
    let dir = tempdir().unwrap();
    StoreIndex::apply_format(dir.path(), StoreIndexFormat::Files).unwrap();
    assert_alias_reads_its_target(dir.path());
}

#[test]
fn an_alias_whose_row_is_missing_reads_as_a_miss() {
    let dir = tempdir().unwrap();
    let mut index = StoreIndex::open(dir.path()).unwrap();
    index
        .set_aliases(&[(TAR_KEY.to_string(), TGZ_KEY.to_string())])
        .unwrap();

    assert_eq!(index.get(TAR_KEY).unwrap(), None);
    assert!(!index.contains_key(TAR_KEY).unwrap());
}

#[test]
fn a_read_only_index_created_before_aliases_reads_no_alias() {
    let dir = tempdir().unwrap();
    let index = StoreIndex::open(dir.path()).unwrap();
    index
        .set(TGZ_KEY, &sample_index())
        .unwrap();
    index
        .sqlite_conn()
        .execute_batch("DROP TABLE package_index_alias;")
        .unwrap();
    drop(index);

    let index = StoreIndex::open_readonly(dir.path()).unwrap();
    assert_eq!(index.get(TAR_KEY).unwrap(), None);
}
