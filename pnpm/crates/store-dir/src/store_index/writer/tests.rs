use super::{WriteMsg, apply_write_msg};
use crate::store_index::{StoreIndex, tests::sample_index};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use tempfile::tempdir;

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
    apply_write_msg(
        &index,
        &mut pending,
        WriteMsg::InvalidateSideEffects {
            key: key.to_string(),
            cache_key: "test-engine".to_string(),
        },
    );
    let mut row = pending.remove(key).unwrap();
    let side_effects = row.side_effects.as_ref().unwrap();
    assert_eq!(side_effects.len(), 1);
    assert!(side_effects.contains_key("other-engine"));
    assert_eq!(row.files, sample_index().files);

    row.algo = "sha256".to_string();
    index.set(key, &row).unwrap();
    apply_write_msg(
        &index,
        &mut pending,
        WriteMsg::InvalidateSideEffects {
            key: key.to_string(),
            cache_key: "other-engine".to_string(),
        },
    );
    row.side_effects = None;
    assert_eq!(pending.remove(key).unwrap(), row);

    apply_write_msg(
        &index,
        &mut pending,
        WriteMsg::InvalidateSideEffects {
            key: "missing-package".to_string(),
            cache_key: "test-engine".to_string(),
        },
    );
    assert!(pending.is_empty());
}

