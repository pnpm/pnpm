//! Canonical key ordering for lockfile maps, plus the YAML serialization entry
//! point.
//!
//! The byte-level YAML rendering lives in [`crate::yaml_emit`], a port of the
//! `@zkochan/js-yaml` dumper pnpm uses for `pnpm-lock.yaml`. This module keeps
//! the `serialize_with` helpers that canonicalize map key order before that
//! rendering runs.

use serde::{
    Serialize,
    ser::{SerializeMap, Serializer},
};
use std::{collections::HashMap, fmt::Display};

/// Serialize `value` to a YAML string matching pnpm's lockfile formatting.
pub(crate) fn to_string<Value: Serialize>(value: &Value) -> Result<String, serde_json::Error> {
    crate::yaml_emit::to_string(value)
}

/// Serialize a [`HashMap`] with its entries emitted in canonical key order.
///
/// pnpm orders every lockfile map by its *rendered* key string via
/// [`lexCompare`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/src/sortLockfileKeys.ts)
/// (a plain code-unit comparison). Sorting by the rendered string — rather
/// than by the key type's structural `Ord` — is load-bearing: the `@`
/// separating `name` from `version` in a `packages:`/`snapshots:` key, and
/// the leading `@` of a scoped `name`, both order differently under a
/// field-wise comparison than under a comparison of the concatenated string
/// (`react-dom@1.0.0` sorts before `react@17.0.2`; `@types/node` sorts before
/// `node`). [`Display`] renders each key exactly as it is serialized, so
/// sorting by it reproduces pnpm's byte order.
pub(crate) fn sorted_map<Key, Value, Ser>(
    map: &HashMap<Key, Value>,
    serializer: Ser,
) -> Result<Ser::Ok, Ser::Error>
where
    Key: Serialize + Display,
    Value: Serialize,
    Ser: Serializer,
{
    let mut entries: Vec<(String, &Key, &Value)> =
        map.iter().map(|(key, value)| (key.to_string(), key, value)).collect();
    entries.sort_unstable_by(|(left, ..), (right, ..)| left.cmp(right));
    let mut map_serializer = serializer.serialize_map(Some(entries.len()))?;
    for (_, key, value) in &entries {
        map_serializer.serialize_entry(key, value)?;
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
    Value: Serialize,
    Ser: Serializer,
{
    match map {
        Some(map) => sorted_map(map, serializer),
        None => serializer.serialize_none(),
    }
}
