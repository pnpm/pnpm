use super::PATCH_HASH_PREFIX;
use crate::PackageKey;

/// The trailing run of balanced, back-to-back parenthesized segments that
/// ends `suffix`, read right to left as pnpm's `parse` reads it, and the text
/// in front of it. A locator such as a `file:` path can hold parentheses of its
/// own. `None` when a segment's opening parenthesis is missing.
pub(super) fn split_suffix(suffix: &str) -> Option<(&str, Vec<&str>)> {
    let bytes = suffix.as_bytes();
    let mut segments = Vec::new();
    let mut end = suffix.len();
    while end > 0 && bytes[end - 1] == b')' {
        let start = matching_open(bytes, end - 1)?;
        segments.push(&suffix[start..end]);
        end = start;
    }
    segments.reverse();
    Some((&suffix[..end], segments))
}

/// The index of the `(` that the `)` at `close` closes.
fn matching_open(bytes: &[u8], close: usize) -> Option<usize> {
    let mut depth = 0;
    for index in (0..=close).rev() {
        match bytes[index] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
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
