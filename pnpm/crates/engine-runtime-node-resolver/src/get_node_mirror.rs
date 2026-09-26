//! Resolves the base URL for a Node.js release channel.

use std::collections::HashMap;

/// Default mirror for the official Node.js releases.
pub const DEFAULT_NODE_MIRROR_BASE_URL: &str = "https://nodejs.org/download/release/";

/// Mirror for the unofficial Node.js builds (musl variants).
pub const UNOFFICIAL_NODE_MIRROR_BASE_URL: &str =
    "https://unofficial-builds.nodejs.org/download/release/";

/// Resolve the base URL for a given release channel.
///
/// The three settings that can name it are read most specific first,
/// and the canonical spelling ahead of the older one where both are as
/// specific:
///
/// 1. `channel`, from `tools.node.channels.<channel>`, which names this
///    channel and no other.
/// 2. `node_download_mirrors`, the older `node-mirror:<channel>`
///    spelling, kept because it has shipped.
/// 3. `mirror`, from `tools.node.mirror`, the base every channel hangs
///    off the way nodejs.org lays its own tree out.
///
/// A channel none of them names falls back to the official nodejs.org
/// tree. The returned URL always ends with `/` so callers can
/// concatenate `v<version>/...` without a defensive check.
#[must_use]
pub fn get_node_mirror(
    mirror: Option<&str>,
    channel: Option<&str>,
    node_download_mirrors: Option<&HashMap<String, String>>,
    release_channel: &str,
) -> String {
    let mirror = channel
        .map(ToString::to_string)
        .or_else(|| node_download_mirrors.and_then(|map| map.get(release_channel).cloned()))
        .or_else(|| mirror.map(|base| format!("{}/{release_channel}", base.trim_end_matches('/'))))
        .unwrap_or_else(|| format!("https://nodejs.org/download/{release_channel}/"));
    normalize_node_mirror(&mirror)
}

fn normalize_node_mirror(mirror: &str) -> String {
    if mirror.ends_with('/') { mirror.to_string() } else { format!("{mirror}/") }
}

#[cfg(test)]
mod tests;
