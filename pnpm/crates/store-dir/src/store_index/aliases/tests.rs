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

fn assert_pkg_id_lookup_follows_an_alias(root: &Path) {
    let mut index = StoreIndex::open(root).unwrap();
    let file_key = "sha512-tgz\tfile:acme-1.0.0.tgz";
    index
        .set(file_key, &sample_index())
        .unwrap();
    assert_eq!(index.get_by_pkg_id("acme@1.0.0").unwrap(), None);

    index
        .set_aliases(&[(TGZ_KEY.to_string(), file_key.to_string())])
        .unwrap();

    assert_eq!(index.get_by_pkg_id("acme@1.0.0").unwrap(), Some(sample_index()));
    assert_eq!(index.get_by_pkg_id("cme@1.0.0").unwrap(), None);
}

#[test]
fn an_sqlite_pkg_id_lookup_follows_an_alias() {
    let dir = tempdir().unwrap();
    assert_pkg_id_lookup_follows_an_alias(dir.path());
}

#[test]
fn a_files_pkg_id_lookup_follows_an_alias() {
    let dir = tempdir().unwrap();
    StoreIndex::apply_format(dir.path(), StoreIndexFormat::Files).unwrap();
    assert_pkg_id_lookup_follows_an_alias(dir.path());
}

#[test]
fn a_pkg_id_lookup_in_a_read_only_index_created_before_aliases_misses() {
    let dir = tempdir().unwrap();
    let index = StoreIndex::open(dir.path()).unwrap();
    index
        .sqlite_conn()
        .execute_batch("DROP TABLE package_index_alias;")
        .unwrap();
    drop(index);

    let index = StoreIndex::open_readonly(dir.path()).unwrap();
    assert_eq!(index.get_by_pkg_id("acme@1.0.0").unwrap(), None);
}

fn assert_pkg_id_lookup_skips_a_dangling_alias(root: &Path) {
    let mut index = StoreIndex::open(root).unwrap();
    let live_key = "sha512-tgz\tfile:acme-1.0.0.tgz";
    index
        .set(live_key, &sample_index())
        .unwrap();
    index
        .set_aliases(&[
            ("sha1-pruned\tacme@1.0.0".to_string(), "sha1-pruned\tfile:gone.tgz".to_string()),
            ("sha512-tgz\tacme@1.0.0".to_string(), live_key.to_string()),
        ])
        .unwrap();

    assert_eq!(index.get_by_pkg_id("acme@1.0.0").unwrap(), Some(sample_index()));
}

#[test]
fn an_sqlite_pkg_id_lookup_skips_a_dangling_alias() {
    let dir = tempdir().unwrap();
    assert_pkg_id_lookup_skips_a_dangling_alias(dir.path());
}

#[test]
fn a_files_pkg_id_lookup_skips_a_dangling_alias() {
    let dir = tempdir().unwrap();
    StoreIndex::apply_format(dir.path(), StoreIndexFormat::Files).unwrap();
    assert_pkg_id_lookup_skips_a_dangling_alias(dir.path());
}

fn assert_pkg_id_lookup_matches_the_name_case(root: &Path) {
    let mut index = StoreIndex::open(root).unwrap();
    index
        .set("sha512-row\tJSONStream@1.0.0", &sample_index())
        .unwrap();
    index
        .set("sha512-tgz\tfile:JSONStream-2.0.0.tgz", &sample_index())
        .unwrap();
    index
        .set_aliases(&[(
            "sha512-tgz\tJSONStream@2.0.0".to_string(),
            "sha512-tgz\tfile:JSONStream-2.0.0.tgz".to_string(),
        )])
        .unwrap();

    assert_eq!(index.get_by_pkg_id("jsonstream@1.0.0").unwrap(), None);
    assert_eq!(index.get_by_pkg_id("jsonstream@2.0.0").unwrap(), None);
    assert_eq!(index.get_by_pkg_id("JSONStream@2.0.0").unwrap(), Some(sample_index()));
}

#[test]
fn an_sqlite_pkg_id_lookup_matches_the_name_case() {
    let dir = tempdir().unwrap();
    assert_pkg_id_lookup_matches_the_name_case(dir.path());
}

#[test]
fn a_files_pkg_id_lookup_matches_the_name_case() {
    let dir = tempdir().unwrap();
    StoreIndex::apply_format(dir.path(), StoreIndexFormat::Files).unwrap();
    assert_pkg_id_lookup_matches_the_name_case(dir.path());
}
