use pacquet_crypto_hash::create_short_hash;

use crate::peer_id::PeerId;

/// Build the peer-suffix that appended to a `pkgIdWithPatchHash` produces
/// a full depPath. Mirrors pnpm's
/// [`createPeerDepGraphHash`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L197-L213).
///
/// `max_length` is upstream's `peersSuffixMaxLength` (default 1000) —
/// the cap is applied to the *body* (segments joined), not the wrapped
/// form, matching the JS comparison `dirName.length > maxLength` before
/// the leading `(` is prepended.
pub fn create_peer_dep_graph_hash(peer_ids: &[PeerId], max_length: usize) -> String {
    let mut segments: Vec<String> = peer_ids.iter().map(PeerId::as_segment).collect();
    segments.sort();
    let body = segments.join(")(");
    if body.len() > max_length {
        format!("({})", create_short_hash(&body))
    } else {
        format!("({body})")
    }
}

#[cfg(test)]
mod tests;
