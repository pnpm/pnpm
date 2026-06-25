//! Port of pnpm's
//! [`createGlobalCacheKey`](https://github.com/pnpm/pnpm/blob/1819226b51/global/packages/src/cacheKey.ts).
//!
//! The key names the on-disk symlink under the global packages directory
//! that points at a group's install directory, so it must hash to exactly
//! the same value pnpm produces for the same aliases + registries.

use pacquet_crypto_hash::create_hex_hash;
use serde_json::{Value, json};

/// Compute the global-install cache key for a group of resolved aliases
/// and the active registry map.
///
/// Mirrors pnpm: the aliases and the registry entries are each sorted
/// lexicographically, JSON-encoded as `[sortedAliases, sortedRegistries]`
/// (each registry entry a `[key, value]` pair), and hashed with sha256
/// (full hex digest).
#[must_use]
pub fn create_global_cache_key(aliases: &[String], registries: &[(String, String)]) -> String {
    let mut sorted_aliases: Vec<&String> = aliases.iter().collect();
    sorted_aliases.sort();
    let mut sorted_registries: Vec<&(String, String)> = registries.iter().collect();
    sorted_registries.sort_by(|a, b| a.0.cmp(&b.0));

    let payload = Value::Array(vec![
        Value::Array(sorted_aliases.iter().map(|alias| json!(alias)).collect()),
        Value::Array(sorted_registries.iter().map(|(key, value)| json!([key, value])).collect()),
    ]);
    // `serde_json::to_string` matches `JSON.stringify`'s compact form
    // (no spaces) for arrays of strings, which is what pnpm hashes.
    let hash_str =
        serde_json::to_string(&payload).expect("JSON array of strings always serializes");
    create_hex_hash(&hash_str)
}

#[cfg(test)]
mod tests;
