use pacquet_crypto_hash::create_short_hash;

use crate::peer_id::PeerId;

/// Build the peer-suffix that appended to a `pkgIdWithPatchHash` produces
/// a full depPath. Mirrors pnpm's
/// [`createPeerDepGraphHash`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L197-L213).
///
/// Rendering rules:
///
/// 1. Each [`PeerId`] becomes one segment via
///    [`PeerId::as_segment`].
/// 2. Segments are sorted lexicographically so the result is stable
///    regardless of resolution order.
/// 3. Segments are joined with `")("`, then wrapped with `(...)` to
///    produce e.g. `(bar@2.0.0)(baz@3.0.0)`.
/// 4. If the joined body exceeds `max_length`, the body is replaced with
///    its short hash (still wrapped in `(...)`).
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
mod tests {
    use super::{PeerId, create_peer_dep_graph_hash};

    fn pair(name: &str, version: &str) -> PeerId {
        PeerId::Pair { name: name.to_string(), version: version.to_string() }
    }

    #[test]
    fn empty_input_renders_a_single_pair_of_parens() {
        let got = create_peer_dep_graph_hash(&[], 1000);
        assert_eq!(got, "()");
    }

    #[test]
    fn pairs_render_as_name_at_version_joined_with_paren_paren() {
        let got = create_peer_dep_graph_hash(&[pair("bar", "2.0.0"), pair("baz", "3.0.0")], 1000);
        assert_eq!(got, "(bar@2.0.0)(baz@3.0.0)");
    }

    #[test]
    fn segments_are_sorted_for_stability() {
        let unsorted =
            create_peer_dep_graph_hash(&[pair("zzz", "1.0.0"), pair("aaa", "1.0.0")], 1000);
        let sorted =
            create_peer_dep_graph_hash(&[pair("aaa", "1.0.0"), pair("zzz", "1.0.0")], 1000);
        assert_eq!(unsorted, sorted);
        assert_eq!(unsorted, "(aaa@1.0.0)(zzz@1.0.0)");
    }

    #[test]
    fn dep_path_strings_with_leading_slash_have_it_stripped() {
        let got =
            create_peer_dep_graph_hash(&[PeerId::DepPath("/foo@1.0.0(bar@2.0.0)".into())], 1000);
        assert_eq!(got, "(foo@1.0.0(bar@2.0.0))");
    }

    #[test]
    fn dep_path_strings_without_leading_slash_pass_through() {
        let got =
            create_peer_dep_graph_hash(&[PeerId::DepPath("foo@1.0.0(bar@2.0.0)".into())], 1000);
        assert_eq!(got, "(foo@1.0.0(bar@2.0.0))");
    }

    #[test]
    fn long_body_is_replaced_with_short_hash() {
        let segments: Vec<PeerId> = (0..50).map(|i| pair(&format!("pkg-{i}"), "1.0.0")).collect();
        let got = create_peer_dep_graph_hash(&segments, 100);
        assert!(got.starts_with('('));
        assert!(got.ends_with(')'));
        let body = &got[1..got.len() - 1];
        assert_eq!(body.len(), 32);
        assert!(body.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
