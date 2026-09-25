use super::PATCH_HASH_PREFIX;
use crate::PackageKey;

/// The top-level parenthesized segments of a depPath suffix, or `None` when
/// its parentheses do not balance.
pub(super) fn top_level_segments(suffix: &str) -> Option<Vec<&str>> {
    let mut depth: i32 = 0;
    let mut start = 0;
    let mut segments = Vec::new();
    for (index, byte) in suffix.bytes().enumerate() {
        match byte {
            b'(' if depth == 0 => {
                start = index;
                depth = 1;
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                if depth == 0 {
                    segments.push(&suffix[start..=index]);
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(segments)
}

/// The peer depPath a suffix segment names when that depPath carries a patch
/// hash of its own: `None` for the package's own hash segment and for a peer
/// with no marker, `Some(Err(()))` for a segment that does not parse as a
/// depPath.
pub(super) fn peer_to_judge(segment: &str) -> Option<Result<PackageKey, ()>> {
    if segment.starts_with(PATCH_HASH_PREFIX) || !segment.contains(PATCH_HASH_PREFIX) {
        return None;
    }
    let inner = segment
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))?;
    Some(
        inner
            .parse::<PackageKey>()
            .map_err(|_| ()),
    )
}
