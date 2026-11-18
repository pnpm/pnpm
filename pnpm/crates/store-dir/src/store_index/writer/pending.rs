use super::{
    CafsFileInfo, HashMap, PackageFilesIndex, SideEffectsDiff, StoreIndex, StoreIndexError,
    WriteMsg,
};

const MAX_QUARANTINED_REMOTE_SIDE_EFFECTS: usize = 64;

pub(super) fn apply_write_msg(
    index: &StoreIndex,
    pending: &mut HashMap<String, PackageFilesIndex>,
    msg: WriteMsg,
) {
    match msg {
        WriteMsg::Replace { key, value } => {
            pending.insert(key, value);
        }
        WriteMsg::SideEffectsUpload {
            key,
            cache_key,
            current_files,
            response,
        } => {
            let diff = load_pending_row(index, pending, &key)
                .and_then(|row| record_local_side_effects(row, &key, cache_key, &current_files));
            if let Some(response) = response {
                let _ = response.send(diff);
            }
        }
        WriteMsg::ReadFilePaths { key, response } => {
            let paths = match pending.get(&key) {
                Some(row) => Some(row.files.keys().cloned().collect()),
                None => load_index_row(index, &key).map(|row| row.files.into_keys().collect()),
            };
            let _ = response.send(paths);
        }
        WriteMsg::RemoteSideEffects { key, cache_key, diff } => {
            if let Some(row) = load_pending_row(index, pending, &key) {
                row.side_effects.get_or_insert_with(HashMap::new).insert(cache_key, diff);
            }
        }
        WriteMsg::InvalidateSideEffects { key, cache_key, response } => {
            let _ = response.send(invalidate_side_effects(index, pending, &key, &cache_key));
        }
        WriteMsg::QuarantineRemoteSideEffects { key, channel, envelope_digest } => {
            if let Some(row) = load_pending_row(index, pending, &key) {
                quarantine_digest(row, channel, envelope_digest);
            }
        }
        WriteMsg::Alias { .. } => unreachable!("flush_batch collects aliases itself"),
    }
}

fn record_local_side_effects(
    row: &mut PackageFilesIndex,
    key: &str,
    cache_key: String,
    current_files: &HashMap<String, CafsFileInfo>,
) -> Option<SideEffectsDiff> {
    if row.algo != crate::upload::HASH_ALGORITHM {
        tracing::warn!(
            target: "pacquet::store_index",
            key = %key,
            row_algo = %row.algo,
            "algo mismatch on base row; skip side-effects upload",
        );
        return None;
    }
    let diff = crate::upload::calculate_diff(&row.files, current_files);
    row.side_effects.get_or_insert_with(HashMap::new).insert(cache_key, diff.clone());
    Some(diff)
}

fn quarantine_digest(row: &mut PackageFilesIndex, channel: String, envelope_digest: String) {
    let digests = row.remote_side_effects_quarantine
        .get_or_insert_with(HashMap::new)
        .entry(channel)
        .or_default();
    if !digests.contains(&envelope_digest) {
        digests.push(envelope_digest);
    }
    if digests.len() > MAX_QUARANTINED_REMOTE_SIDE_EFFECTS {
        digests.drain(..digests.len() - MAX_QUARANTINED_REMOTE_SIDE_EFFECTS);
    }
}

pub(super) fn invalidate_side_effects(
    index: &StoreIndex,
    pending: &mut HashMap<String, PackageFilesIndex>,
    key: &str,
    cache_key: &str,
) -> Result<(), StoreIndexError> {
    if let Some(row) = pending.get_mut(key) {
        remove_side_effects(row, cache_key);
        index.set(key, row)?;
        pending.remove(key);
    } else if let Some(mut row) = index.get(key)?
        && remove_side_effects(&mut row, cache_key)
    {
        index.set(key, &row)?;
    }
    Ok(())
}

fn remove_side_effects(row: &mut PackageFilesIndex, cache_key: &str) -> bool {
    let Some(side_effects) = &mut row.side_effects else {
        return false;
    };
    if side_effects.remove(cache_key).is_none() {
        return false;
    }
    if side_effects.is_empty() {
        row.side_effects = None;
    }
    true
}

fn load_pending_row<'a>(
    index: &StoreIndex,
    pending: &'a mut HashMap<String, PackageFilesIndex>,
    key: &str,
) -> Option<&'a mut PackageFilesIndex> {
    use std::collections::hash_map::Entry;
    match pending.entry(key.to_string()) {
        Entry::Occupied(o) => Some(o.into_mut()),
        Entry::Vacant(v) => load_index_row(index, key).map(|row| v.insert(row)),
    }
}

fn load_index_row(index: &StoreIndex, key: &str) -> Option<PackageFilesIndex> {
    match index.get(key) {
        Ok(Some(row)) => Some(row),
        Ok(None) => {
            tracing::debug!(target: "pacquet::store_index", key = %key, "no base row for side-effects upload; skip");
            None
        }
        Err(error) => {
            tracing::warn!(target: "pacquet::store_index", ?error, key = %key, "failed to read base row for side-effects upload");
            None
        }
    }
}
