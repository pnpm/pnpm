use pnpm_crypto_hash::create_short_hash;

use crate::peer_id::PeerId;

/// Build the peer-suffix that appended to a `pkgIdWithPatchHash` produces
/// a full depPath.
///
/// `max_length` is the `peersSuffixMaxLength` (default 1000), the cap
/// is applied to the *body* (segments joined), not the wrapped form,
/// so the comparison `dirName.length > maxLength` happens before the
/// leading `(` is prepended.
pub fn create_peer_dep_graph_hash(peer_ids: &[PeerId], max_length: usize) -> String {
    let mut segments: Vec<String> = peer_ids
        .iter()
        .map(PeerId::to_segment)
        .collect();
    segments.sort();
    if segments.iter().any(|segment| file_path_has_parentheses(segment)) {
        // NUL cannot occur in a path and keeps peer boundaries distinct from literal ")(".
        let framed = format!("\0{}", segments.join("\0"));
        return format!("({})", create_short_hash(&framed));
    }
    let body = segments.join(")(");
    if body.len() > max_length {
        format!("({})", create_short_hash(&body))
    } else {
        format!("({body})")
    }
}

fn file_path_has_parentheses(segment: &str) -> bool {
    let Some(at) = crate::version_separator_index(segment) else { return false };
    let Some(path) = segment[at + 1..].strip_prefix("file:") else { return false };
    crate::remove_suffix(path).contains(['(', ')'])
}

#[cfg(test)]
mod tests;
