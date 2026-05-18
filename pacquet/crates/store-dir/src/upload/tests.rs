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

/// Identical maps produce an empty diff. The `added` and `deleted`
/// fields stay `None` (not `Some(empty)`) so the msgpack payload
/// elides them entirely — matches the `if (deleted.length > 0)` /
/// `if (added.size > 0)` guards upstream.
#[test]
fn identical_maps_yield_no_diff() {
    let m = map(&[("a", info("d-a", 0o644, 1))]);
    let diff = calculate_diff(&m, &m);
    assert_eq!(diff.added, None);
    assert_eq!(diff.deleted, None);
}

/// New file present only in `current` lands under `added`.
#[test]
fn added_only() {
    let base = HashMap::new();
    let current = map(&[("new", info("d-new", 0o644, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert!(added.contains_key("new"));
}

/// File present in `base` and missing in `current` lands under
/// `deleted` (file removed by the postinstall).
#[test]
fn deleted_only() {
    let base = map(&[("gone", info("d-gone", 0o644, 1))]);
    let current = HashMap::new();
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.added, None);
    let deleted = diff.deleted.expect("deleted present");
    assert_eq!(deleted, vec!["gone".to_string()]);
}

/// Digest change at the same path appears under `added`, not `deleted`.
/// Mirrors the `baseFiles.get(file)!.digest !== sideEffectsFiles.get(file)!.digest`
/// branch at upstream's calculateDiff:418-421.
#[test]
fn digest_change_appears_in_added() {
    let base = map(&[("f.txt", info("d-old", 0o644, 1))]);
    let current = map(&[("f.txt", info("d-new", 0o644, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert_eq!(added.get("f.txt").unwrap().digest, "d-new");
}

/// Mode change at the same path (and same digest) appears under
/// `added` — upstream catches this via the `baseFiles.get(file)!.mode
/// !== sideEffectsFiles.get(file)!.mode` branch.
#[test]
fn mode_change_appears_in_added() {
    let base = map(&[("f.sh", info("d", 0o644, 1))]);
    let current = map(&[("f.sh", info("d", 0o755, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert_eq!(added.get("f.sh").unwrap().mode, 0o755);
}

/// Mixed: one delete, one add, one mod, one unchanged.
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
