use super::calculate_diff;
use crate::CafsFileInfo;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

fn info(digest: &str, mode: u32, size: u64) -> CafsFileInfo {
    CafsFileInfo { digest: digest.to_string(), mode, size, checked_at: None }
}

fn map(entries: &[(&str, CafsFileInfo)]) -> HashMap<String, CafsFileInfo> {
    entries
        .iter()
        .map(|(k, v)| {
            (
                (*k).to_string(),
                CafsFileInfo {
                    digest: v.digest.clone(),
                    mode: v.mode,
                    size: v.size,
                    checked_at: v.checked_at,
                },
            )
        })
        .collect()
}

#[test]
fn identical_maps_yield_no_diff() {
    let files = map(&[("a", info("d-a", 0o644, 1))]);
    let diff = calculate_diff(&files, &files);
    assert_eq!(diff.added, None);
    assert_eq!(diff.deleted, None);
}

#[test]
fn added_only() {
    let base = HashMap::new();
    let current = map(&[("new", info("d-new", 0o644, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert!(added.contains_key("new"));
}

#[test]
fn deleted_only() {
    let base = map(&[("gone", info("d-gone", 0o644, 1))]);
    let current = HashMap::new();
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.added, None);
    let deleted = diff.deleted.expect("deleted present");
    assert_eq!(deleted, vec!["gone".to_string()]);
}

#[test]
fn digest_change_appears_in_added() {
    let base = map(&[("f.txt", info("d-old", 0o644, 1))]);
    let current = map(&[("f.txt", info("d-new", 0o644, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert_eq!(added.get("f.txt").unwrap().digest, "d-new");
}

#[test]
fn mode_change_appears_in_added() {
    let base = map(&[("f.sh", info("d", 0o644, 1))]);
    let current = map(&[("f.sh", info("d", 0o755, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert_eq!(added.get("f.sh").unwrap().mode, 0o755);
}

#[test]
fn mixed_changes() {
    let base = map(&[
        ("keep", info("d-keep", 0o644, 1)),
        ("gone", info("d-gone", 0o644, 1)),
        ("changed", info("d-old", 0o644, 1)),
    ]);
    let current = map(&[
        ("keep", info("d-keep", 0o644, 1)),
        ("changed", info("d-new", 0o644, 1)),
        ("fresh", info("d-fresh", 0o644, 1)),
    ]);
    let diff = calculate_diff(&base, &current);
    let added = diff.added.expect("added present");
    let mut added_keys: Vec<_> = added.keys().cloned().collect();
    added_keys.sort();
    assert_eq!(added_keys, vec!["changed".to_string(), "fresh".to_string()]);
    assert_eq!(diff.deleted, Some(vec!["gone".to_string()]));
}
