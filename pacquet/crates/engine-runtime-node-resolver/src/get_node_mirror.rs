//! Pacquet port of
//! [`getNodeMirror.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/getNodeMirror.ts).

use std::collections::HashMap;

/// Default mirror for the official Node.js releases.
///
/// Mirrors upstream's
/// [`DEFAULT_NODE_MIRROR_BASE_URL`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/index.ts#L25).
pub const DEFAULT_NODE_MIRROR_BASE_URL: &str = "https://nodejs.org/download/release/";

/// Mirror for the unofficial Node.js builds (musl variants).
///
/// Mirrors upstream's
/// [`UNOFFICIAL_NODE_MIRROR_BASE_URL`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/index.ts#L26).
pub const UNOFFICIAL_NODE_MIRROR_BASE_URL: &str =
    "https://unofficial-builds.nodejs.org/download/release/";

/// Resolve the base URL for a given release channel.
///
/// `node_download_mirrors` is the user's `.npmrc`/config override map
/// keyed by channel (`release`, `nightly`, `rc`, `test`, `v8-canary`).
/// A missing entry falls back to the official nodejs.org tree. The
/// returned URL always ends with `/` so callers can concatenate
/// `v<version>/...` without a defensive check.
#[must_use]
pub fn get_node_mirror(
    node_download_mirrors: Option<&HashMap<String, String>>,
    release_channel: &str,
) -> String {
    let mirror = node_download_mirrors
        .and_then(|map| map.get(release_channel).cloned())
        .unwrap_or_else(|| format!("https://nodejs.org/download/{release_channel}/"));
    normalize_node_mirror(&mirror)
}

fn normalize_node_mirror(mirror: &str) -> String {
    if mirror.ends_with('/') { mirror.to_string() } else { format!("{mirror}/") }
}

#[cfg(test)]
mod tests;
