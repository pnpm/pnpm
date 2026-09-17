//! Resolves the base URL for a Node.js release channel.

use std::collections::HashMap;

/// Default mirror for the official Node.js releases.
pub const DEFAULT_NODE_MIRROR_BASE_URL: &str = "https://nodejs.org/download/release/";

/// Mirror for the unofficial Node.js builds (musl variants).
pub const UNOFFICIAL_NODE_MIRROR_BASE_URL: &str =
    "https://unofficial-builds.nodejs.org/download/release/";

/// Resolve the base URL for a given release channel.
///
/// `mirror` is `tools.node.mirror`, the base the channels hang off the
/// way nodejs.org lays them out. `node_download_mirrors` is the older
/// `node-mirror:<channel>` spelling, kept because it has shipped, and it
/// wins for a channel it names: it says where one channel comes from,
/// where the base says where all of them do.
///
/// The returned URL always ends with `/` so callers can concatenate
/// `v<version>/...` without a defensive check.
#[must_use]
pub fn get_node_mirror(
    mirror: Option<&str>,
    node_download_mirrors: Option<&HashMap<String, String>>,
    release_channel: &str,
) -> String {
    let mirror = node_download_mirrors
        .and_then(|map| map.get(release_channel).cloned())
        .or_else(|| mirror.map(|base| format!("{}/{release_channel}", base.trim_end_matches('/'))))
        .unwrap_or_else(|| format!("https://nodejs.org/download/{release_channel}/"));
    normalize_node_mirror(&mirror)
}

fn normalize_node_mirror(mirror: &str) -> String {
    if mirror.ends_with('/') { mirror.to_string() } else { format!("{mirror}/") }
}

#[cfg(test)]
mod tests;
