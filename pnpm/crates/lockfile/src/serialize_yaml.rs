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
/// The lowering to a [`serde_json::Value`] tree is where a
/// workspace-scale lockfile spends this function's time, almost all of
/// it inside the big sorted maps (`importers:`, `packages:`,
/// `snapshots:`), whose entries lower independently. While this
/// function runs, [`sorted_map`] lowers any large map's values across
/// rayon and stashes the assembled object here, leaving a marker
/// string in its place; the markers are spliced back below by *moving*
/// the stashed objects in, so the document is identical to what the
/// plain serialization builds.
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
        debug_assert_eq!(remaining, 0, "every stashed map must have a marker in the document");
    }
    crate::prune_time(&mut document);
    Ok(crate::yaml_emit::to_string(document))
}

/// Entry threshold below which [`sorted_map`] keeps the plain serial
/// lowering: fanning a small map across rayon costs more than it saves,
/// and only workspace-scale maps matter.
const PARALLEL_LOWERING_THRESHOLD: usize = 64;

/// Marker prefix for stashed-map placeholders. The private-use
/// character keeps it out of any well-formed lockfile string, and the
/// per-call id distinguishes concurrent [`to_string`] calls' stashes.
/// Lockfile strings are attacker-influenced, so the id also carries a
/// nanosecond timestamp: a crafted string cannot predict the marker and
/// steal a splice. [`splice_lowered_maps`] additionally leaves any
/// prefix-matching string that doesn't name an unconsumed stash entry
/// alone rather than trusting it.
const STASH_MARKER_PREFIX: &str = "\u{f8ff}pacquet-lowered-map:";

fn next_stash_id() -> u64 {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| u64::from(elapsed.subsec_nanos()));
    (nanos << 32) | COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Maps lowered in parallel during the current [`to_string`] call on
/// this thread, waiting to be spliced over their markers. `None`
/// outside a [`to_string`] call — the state [`sorted_map`] checks to know
/// whether the parallel path is available.
struct LoweredMaps {
    nonce: String,
    maps: Vec<Option<serde_json::Value>>,
}

thread_local! {
    static LOWERED_MAPS: RefCell<Option<LoweredMaps>> = const { RefCell::new(None) };
}

/// Replace every stashed-map marker under `node` with its stashed
/// object, moving rather than rebuilding it. Markers only occur in the
/// shell the serial lowering built — never inside a stashed map, whose
/// entries were lowered with the stash taken away — so the walk skips
/// spliced values and stops once every stash entry has landed.
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
        && let Some(marker) =
            stash_parallel_lowered(&entries).map_err(serde::ser::Error::custom)?
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
