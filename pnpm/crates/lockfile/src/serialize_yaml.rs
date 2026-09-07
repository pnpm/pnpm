//! Canonical key ordering for lockfile maps, plus the YAML serialization entry
//! point.
//!
//! The byte-level YAML rendering lives in [`crate::yaml_emit`], a port of the
//! `@zkochan/js-yaml` dumper pnpm uses for `pnpm-lock.yaml`. This module keeps
//! the `serialize_with` helpers that canonicalize map key order before that
//! rendering runs, plus the on-write normalization pnpm applies to the
//! document as a whole.

use rayon::prelude::*;
use serde::{
    Serialize,
    ser::{SerializeMap, Serializer},
};
use std::{cell::RefCell, collections::HashMap, fmt::Display};

/// Serialize `value` to a YAML string matching pnpm's lockfile formatting.
///
/// The document is normalized before rendering, the way pnpm's
/// `normalizeLockfile` normalizes a lockfile on its way to disk.
///
/// Workspace-scale maps are lowered to [`serde_json::Value`] in
/// parallel (see [`sorted_map`]); the output is byte-identical to the
/// serial lowering.
pub(crate) fn to_string<Document: Serialize>(
    value: &Document,
) -> Result<String, serde_json::Error> {
    let stash_nonce = format!("{STASH_MARKER_PREFIX}{}:", next_stash_id());
    // Replace-and-restore rather than assume `None`, so a nested
    // serialization (should one ever appear) cannot clobber its
    // caller's stash.
    let previous = LOWERED_MAPS.with_borrow_mut(|stash| {
        stash.replace(LoweredMaps { nonce: stash_nonce.clone(), maps: Vec::new() })
    });
    let document = serde_json::to_value(value);
    let stash = LOWERED_MAPS.with_borrow_mut(|slot| std::mem::replace(slot, previous));
    let mut document = document?;
    let mut maps = stash.expect("the stash installed above is only taken here").maps;
    if !maps.is_empty() {
        let mut remaining = maps.len();
        splice_lowered_maps(&mut document, &stash_nonce, &mut maps, &mut remaining);
        if remaining != 0 {
            // A stashed map's marker never surfaced — a logic bug this
            // module must not turn into a corrupt lockfile. The stash
            // is inactive now, so this re-serialization takes the plain
            // serial path and is correct regardless.
            document = serde_json::to_value(value)?;
        }
    }
    crate::prune_time(&mut document);
    Ok(crate::yaml_emit::to_string(document))
}

/// Entry threshold below which [`sorted_map`] keeps the plain serial
/// lowering: fanning a small map across rayon costs more than it saves,
/// and only workspace-scale maps matter.
const PARALLEL_LOWERING_THRESHOLD: usize = 64;

/// Marker prefix for stashed-map placeholders.
const STASH_MARKER_PREFIX: &str = "\u{f8ff}pacquet-lowered-map:";

/// Unpredictable per-call marker id. Lockfile strings are
/// attacker-influenced, so a guessable id would let a crafted string
/// pose as a marker and steal a splice; hashing a counter through a
/// process-random [`std::hash::RandomState`] leaves nothing to
/// enumerate.
fn next_stash_id() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static RANDOM: std::sync::LazyLock<std::hash::RandomState> =
        std::sync::LazyLock::new(std::hash::RandomState::new);
    let mut hasher = RANDOM.build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    hasher.finish()
}

/// Maps lowered in parallel during the current [`to_string`] call on
/// this thread, waiting to be spliced over their markers.
struct LoweredMaps {
    nonce: String,
    maps: Vec<Option<serde_json::Value>>,
}

thread_local! {
    static LOWERED_MAPS: RefCell<Option<LoweredMaps>> = const { RefCell::new(None) };
}

/// Replace every stashed-map marker under `node` with its stashed
/// object, moving rather than rebuilding it. The walk does not descend
/// into spliced values (a stashed map never contains a marker) and
/// stops once `remaining` hits zero.
fn splice_lowered_maps(
    node: &mut serde_json::Value,
    nonce: &str,
    maps: &mut [Option<serde_json::Value>],
    remaining: &mut usize,
) {
    match node {
        serde_json::Value::String(marker) if marker.starts_with(nonce) => {
            // A string that carries the nonce but doesn't name an
            // unconsumed stash entry is not one of ours (lockfile
            // strings are attacker-influenced); leave it as data.
            let Some(stashed) = marker[nonce.len()..]
                .parse::<usize>()
                .ok()
                .and_then(|index| maps.get_mut(index))
                .and_then(Option::take)
            else {
                return;
            };
            *node = stashed;
            *remaining -= 1;
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                if *remaining == 0 {
                    return;
                }
                splice_lowered_maps(value, nonce, maps, remaining);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                if *remaining == 0 {
                    return;
                }
                splice_lowered_maps(value, nonce, maps, remaining);
            }
        }
        _ => {}
    }
}

/// How many maps [`stash_parallel_lowered`] has lowered in this
/// process. Lets tests assert the parallel path actually ran, not just
/// that its output matched.
pub(crate) static PARALLEL_LOWERINGS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Lower a large sorted map's values across rayon and stash the
/// assembled object, returning the marker to serialize in its place.
/// `None` when no [`to_string`] stash is active on this thread — the
/// caller then keeps the plain serial path.
///
/// The stash is taken off the thread for the duration of the parallel
/// lowering: rayon may run some closures inline on this thread, and a
/// nested large map inside an entry must lower serially into its own
/// entry rather than stash itself into the outer document.
fn stash_parallel_lowered<Value: Serialize + Sync>(
    entries: &[(String, &Value)],
) -> Result<Option<String>, serde_json::Error> {
    let Some(mut stash) = LOWERED_MAPS.with_borrow_mut(Option::take) else {
        return Ok(None);
    };
    PARALLEL_LOWERINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let lowered: Result<Vec<serde_json::Value>, serde_json::Error> =
        entries.par_iter().map(|(_, value)| serde_json::to_value(value)).collect();
    let result = lowered.map(|lowered| {
        let mut map = serde_json::Map::with_capacity(entries.len());
        for ((key, _), value) in entries.iter().zip(lowered) {
            map.insert(key.clone(), value);
        }
        let marker = format!("{}{}", stash.nonce, stash.maps.len());
        stash.maps.push(Some(serde_json::Value::Object(map)));
        marker
    });
    LOWERED_MAPS.with_borrow_mut(|slot| *slot = Some(stash));
    result.map(Some)
}

/// Serialize a [`HashMap`] with its entries emitted in canonical key order.
///
/// Every lockfile map is ordered by its *rendered* key string under a plain
/// code-unit comparison. Sorting by the rendered string — rather than by the
/// key type's structural `Ord` — is load-bearing: the `@` separating `name`
/// from `version` in a `packages:`/`snapshots:` key, and the leading `@` of a
/// scoped `name`, both order differently under a field-wise comparison than
/// under a comparison of the concatenated string (`react-dom@1.0.0` sorts
/// before `react@17.0.2`; `@types/node` sorts before `node`). [`Display`]
/// renders each key exactly as it is serialized — every key type here
/// serializes `into = "String"` by rendering its [`Display`] form — so
/// the one rendering serves both the sort and the emitted key.
pub(crate) fn sorted_map<Key, Value, Ser>(
    map: &HashMap<Key, Value>,
    serializer: Ser,
) -> Result<Ser::Ok, Ser::Error>
where
    Key: Serialize + Display,
    Value: Serialize + Sync,
    Ser: Serializer,
{
    let mut entries: Vec<(String, &Value)> =
        map.iter().map(|(key, value)| (key.to_string(), value)).collect();
    entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
    if entries.len() >= PARALLEL_LOWERING_THRESHOLD
        && let Some(marker) = stash_parallel_lowered(&entries).map_err(serde::ser::Error::custom)?
    {
        return serializer.serialize_str(&marker);
    }
    let mut map_serializer = serializer.serialize_map(Some(entries.len()))?;
    for (key, value) in &entries {
        map_serializer.serialize_entry(key.as_str(), value)?;
    }
    map_serializer.end()
}

/// [`sorted_map`] for an `Option<HashMap<…>>` field. The `None` arm is
/// unreachable in practice — every call site pairs this with
/// `skip_serializing_if = "Option::is_none"` — but is handled so the helper
/// is a drop-in `serialize_with` for optional maps.
#[expect(clippy::ref_option, reason = "serde serialize_with is invoked as f(&field, serializer)")]
pub(crate) fn sorted_map_opt<Key, Value, Ser>(
    map: &Option<HashMap<Key, Value>>,
    serializer: Ser,
) -> Result<Ser::Ok, Ser::Error>
where
    Key: Serialize + Display,
    Value: Serialize + Sync,
    Ser: Serializer,
{
    match map {
        Some(map) => sorted_map(map, serializer),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests;
