use super::{WriteMsg, apply_write_msg, invalidate_side_effects};
use crate::store_index::{SideEffectsDiff, StoreIndex, tests::sample_index};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use tempfile::tempdir;

#[test]
fn side_effects_invalidation_does_not_queue_unchanged_rows() {
    let dir = tempdir().unwrap();
    let index = StoreIndex::open(dir.path()).unwrap();
    let key = "built-package";
    for side_effects in [
        None,
        Some(HashMap::new()),
        Some(HashMap::from([(
            "other-engine".to_string(),
            SideEffectsDiff { added: None, deleted: None, remote_origin: None },
        )])),
    ] {
        let mut row = sample_index();
        row.side_effects = side_effects;
        index.set(key, &row).unwrap();
        let mut pending = HashMap::new();
        let writes = index.conn.total_changes();
        invalidate_side_effects(&index, &mut pending, key, "test-engine").unwrap();
        dbg!(&row, &pending);
        assert!(pending.is_empty());
        assert_eq!(index.conn.total_changes(), writes);

        row.requires_build = Some(true);
        let mut replacement = index.get(key).unwrap().unwrap();
        replacement.requires_build = Some(true);
        apply_write_msg(
            &index,
            &mut pending,
            WriteMsg::Replace { key: key.to_string(), value: replacement },
        );
        invalidate_side_effects(&index, &mut pending, key, "test-engine").unwrap();
        dbg!(&pending);
        assert!(pending.is_empty());
        assert_eq!(index.get(key).unwrap().unwrap(), row);
    }
}

#[test]
fn side_effects_invalidation_preserves_pending_uploads_and_ignores_algorithm_mismatch() {
    let dir = tempdir().unwrap();
    let index = StoreIndex::open(dir.path()).unwrap();
    let key = "built-package";
    let mut pending = HashMap::new();
    apply_write_msg(
        &index,
        &mut pending,
        WriteMsg::Replace { key: key.to_string(), value: sample_index() },
    );
    for cache_key in ["test-engine", "other-engine"] {
        apply_write_msg(
            &index,
            &mut pending,
            WriteMsg::SideEffectsUpload {
                key: key.to_string(),
                cache_key: cache_key.to_string(),
                current_files: sample_index().files,
                response: None,
            },
        );
    }
    invalidate_side_effects(&index, &mut pending, key, "test-engine").unwrap();
    assert!(pending.is_empty());
    let mut row = index.get(key).unwrap().unwrap();
    let side_effects = row.side_effects.as_ref().unwrap();
    assert_eq!(side_effects.len(), 1);
    assert!(side_effects.contains_key("other-engine"));
    assert_eq!(row.files, sample_index().files);

    row.algo = "sha256".to_string();
    index.set(key, &row).unwrap();
    invalidate_side_effects(&index, &mut pending, key, "other-engine").unwrap();
    row.side_effects = None;
    assert!(pending.is_empty());
    assert_eq!(index.get(key).unwrap().unwrap(), row);

    invalidate_side_effects(&index, &mut pending, "missing-package", "test-engine").unwrap();
    assert!(pending.is_empty());
}
