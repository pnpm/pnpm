//! Paths inside a package: archive entries and declared bin targets, split
//! into segments that cannot leave the package root.
//!
//! Creusot proves, see `pnpm/VERIFICATION.md`, that no function here panics
//! and that every segment a function keeps or joins is a name: non-empty,
//! neither `.` nor `..`, and holding no `/` or `\`. Archive entry segments
//! also hold no Windows drive.

#[cfg(creusot)]
pub mod spec;

#[cfg(creusot)]
use creusot_std::prelude::{
    Seq,
    ensures,
    invariant,
    proof_assert,
};
#[cfg(creusot)]
use pnpm_creusot_models as _;
#[cfg(creusot)]
use spec::{
    all_driveless_names,
    all_names,
    is_current,
    is_drive_prefixed,
    is_entry_segment,
    is_parent,
    joined,
    joined_step,
    separator_free,
};
use split_first_char::SplitFirstChar;

/// The separators a path splits on. `\` is one, as it is in pnpm's
/// `parseTarball.ts`, so a path built on Windows means the same everywhere.
const SEPARATORS: [char; 2] = ['/', '\\'];

/// What one segment between separators names.
enum SegmentKind {
    Empty,
    Current,
    Parent,
    Name,
}

#[cfg_attr(creusot, ensures(match result {
    SegmentKind::Empty => segment@.len() == 0,
    SegmentKind::Current => is_current(segment@),
    SegmentKind::Parent => is_parent(segment@),
    SegmentKind::Name => segment@.len() > 0 && !is_current(segment@) && !is_parent(segment@),
}))]
fn segment_kind(segment: &str) -> SegmentKind {
    let Some((first, after_first)) = segment.split_first_char() else {
        return SegmentKind::Empty;
    };
    if first != '.' {
        return SegmentKind::Name;
    }
    let Some((second, after_second)) = after_first.split_first_char() else {
        #[cfg(creusot)]
        proof_assert! { after_first@.ext_eq(Seq::empty()) };
        #[cfg(creusot)]
        proof_assert! { Seq::singleton('.').concat_empty(); segment@ == Seq::singleton('.') };
        return SegmentKind::Current;
    };
    #[cfg(creusot)]
    proof_assert! { segment@[1] == second && Seq::singleton('.').concat(Seq::singleton('.'))[1] == '.' };
    if second != '.' || after_second.split_first_char().is_some() {
        return SegmentKind::Name;
    }
    #[cfg(creusot)]
    proof_assert! { after_second@.ext_eq(Seq::empty()) };
    #[cfg(creusot)]
    proof_assert! {
        Seq::singleton('.').concat_empty();
        segment@ == Seq::singleton('.').concat(Seq::singleton('.'))
    };
    SegmentKind::Parent
}

/// Whether `path` starts with a Windows drive, such as `C:`.
#[cfg_attr(creusot, ensures(result == is_drive_prefixed(path@)))]
#[must_use]
pub fn has_windows_drive_prefix(path: &str) -> bool {
    let Some((drive, rest)) = path.split_first_char() else {
        return false;
    };
    let Some((colon, _)) = rest.split_first_char() else {
        return false;
    };
    drive.is_ascii_alphabetic() && colon == ':'
}

/// The next segment of `rest`, and what follows its separator if one ends it.
#[cfg_attr(creusot, ensures(separator_free(result.0@)))]
fn next_segment(rest: &str) -> (&str, Option<&str>) {
    match rest.split_once(SEPARATORS) {
        Some((segment, tail)) => (segment, Some(tail)),
        None => (rest, None),
    }
}

/// Split a published archive entry's path into its segments, rejecting
/// anything that escapes the archive root.
///
/// `\` is a separator, as pnpm treats it before it validates
/// (`parseTarball.ts`). Without that, a Windows-built entry keeps its
/// backslashes verbatim on Unix (where they are ordinary filename
/// characters) and the resulting key travels through the `index.db`
/// both implementations share to a reader that *does* treat them as
/// separators.
///
/// A leading `.` is preserved because npm's `tar` counts it as the
/// component removed by `strip: 1`. Other `.` components are ignored.
///
/// `None` for an absolute path, one climbing past the root, or one naming a
/// Windows drive.
#[cfg_attr(creusot, ensures(match result {
    None => true,
    Some(segments) => segments@.len() > 0
        && forall<index> 0 <= index && index < segments@.len() ==>
            is_entry_segment(segments@[index]@) && (is_current(segments@[index]@) ==> index == 0),
}))]
#[must_use]
pub fn archive_entry_segments(raw: &str) -> Option<Vec<&str>> {
    if raw.starts_with(SEPARATORS) {
        return None;
    }
    let mut segments: Vec<&str> = Vec::new();
    let mut rest = raw;
    let mut at_start = true;
    #[cfg_attr(creusot, invariant(at_start ==> segments@.len() == 0))]
    #[cfg_attr(creusot, invariant(forall<index> 0 <= index && index < segments@.len() ==>
        is_entry_segment(segments@[index]@) && (is_current(segments@[index]@) ==> index == 0)))]
    loop {
        let (segment, tail) = next_segment(rest);
        match segment_kind(segment) {
            SegmentKind::Empty => {}
            SegmentKind::Current if at_start => segments.push(segment),
            SegmentKind::Current => {}
            SegmentKind::Parent => return None,
            SegmentKind::Name if has_windows_drive_prefix(segment) => return None,
            SegmentKind::Name => segments.push(segment),
        }
        at_start = false;
        let Some(tail) = tail else { break };
        rest = tail;
    }
    if segments.is_empty() { None } else { Some(segments) }
}

/// Why an archive entry path has no clean form.
#[derive(Debug, Clone, Copy)]
pub enum EntryPathRejection {
    /// The path is absolute, climbs past the archive root, or names a
    /// Windows drive.
    NonNormalComponent,
    /// The path is `.`, the archive root itself.
    ArchiveRoot,
}

/// Validate and clean one archive entry path: reject traversal, drop
/// the top-level package directory (`package/`), and join the remaining
/// segments with forward slashes.
///
/// Rejected rather than normalized so a tampered tarball is visible
/// instead of silently landing outside the store.
///
/// An entry that is only one segment long keeps that segment. Such an
/// entry sits at the archive root (beside `package/`, or in a flat
/// archive with no wrapping directory at all) so there is no
/// top-level directory on it to drop, and pnpm keys it by its own name
/// (`parseString` in `parseTarball.ts` advances past the first
/// separator, which a single segment has none of). Dropping the segment
/// instead would leave nothing to key the file by, and rejecting the
/// entry would fail an archive that every other installer accepts. A
/// lone `.` is the exception: it names the archive root rather than
/// anything inside it, so there is no file for a key to address.
///
/// Joined by hand rather than with `PathBuf`, whose native separator
/// would desynchronize these keys from pnpm's always-forward-slashed
/// path layer and the `index.db` both implementations share.
#[cfg_attr(creusot, ensures(match result {
    Ok(path) => exists<segments: Seq<&str>>
        path@ == joined(segments) && all_driveless_names(segments),
    Err(_) => true,
}))]
pub fn clean_archive_entry_path(raw: &str) -> Result<String, EntryPathRejection> {
    let Some(mut segments) = archive_entry_segments(raw) else {
        return Err(EntryPathRejection::NonNormalComponent);
    };
    if segments.len() > 1 {
        segments.remove(0);
    } else if matches!(segment_kind(segments[0]), SegmentKind::Current) {
        return Err(EntryPathRejection::ArchiveRoot);
    }
    #[cfg(creusot)]
    proof_assert! { all_driveless_names(segments@) };
    let path = join(&segments);
    #[cfg(creusot)]
    proof_assert! { path@ == joined(segments@) && all_driveless_names(segments@) };
    Ok(path)
}

/// What a `..` that would climb past the root does.
#[derive(Clone, Copy)]
enum ParentTraversal {
    /// Rejects the path.
    Reject,
    /// Stays at the root.
    Clamp,
}

#[cfg_attr(creusot, ensures(match result {
    Some(path) => exists<segments: Seq<&str>>
        path@ == joined(segments) && all_names(segments),
    None => true,
}))]
fn normalize_relative_path(raw: &str, parent_traversal: ParentTraversal) -> Option<String> {
    if raw.starts_with(SEPARATORS) {
        return None;
    }
    let mut stack: Vec<&str> = Vec::new();
    let mut rest = raw;
    #[cfg_attr(creusot, invariant(all_names(stack@)))]
    loop {
        let (segment, tail) = next_segment(rest);
        match segment_kind(segment) {
            SegmentKind::Empty | SegmentKind::Current => {}
            SegmentKind::Parent => {
                if stack.pop().is_none() && matches!(parent_traversal, ParentTraversal::Reject) {
                    return None;
                }
            }
            SegmentKind::Name => stack.push(segment),
        }
        let Some(tail) = tail else { break };
        rest = tail;
    }
    let path = join(&stack);
    #[cfg(creusot)]
    proof_assert! { path@ == joined(stack@) && all_names(stack@) };
    Some(path)
}

/// Normalize a package-relative path using `/` separators, rejecting paths
/// that start with `/`, contain `\` or `:`, or climb above the package root
/// via `..`.
#[cfg_attr(creusot, ensures(match result {
    Some(path) => exists<segments: Seq<&str>>
        path@ == joined(segments) && all_names(segments),
    None => true,
}))]
#[must_use]
pub fn normalize_package_relative_path(raw: &str) -> Option<String> {
    if raw.contains([':', '\\']) {
        return None;
    }
    normalize_relative_path(raw, ParentTraversal::Reject)
}

/// Normalize a declared bin target within the package root using `/`
/// separators. Excess parent segments are clamped at that root.
#[cfg_attr(creusot, ensures(match result {
    Some(path) => !is_drive_prefixed(path@)
        && exists<segments: Seq<&str>>
            path@ == joined(segments) && all_names(segments),
    None => true,
}))]
#[must_use]
pub fn normalize_bin_path(raw: &str) -> Option<String> {
    if has_windows_drive_prefix(raw) {
        return None;
    }
    let normalized = normalize_relative_path(raw, ParentTraversal::Clamp)?;
    if has_windows_drive_prefix(&normalized) { None } else { Some(normalized) }
}

#[cfg_attr(creusot, ensures(result@ == joined(segments@)))]
fn join(segments: &[&str]) -> String {
    let mut path = String::new();
    let mut index = 0;
    #[cfg_attr(creusot, invariant(index@ <= segments@.len()))]
    #[cfg_attr(creusot, invariant(path@ == joined(segments@.subsequence(0, index@))))]
    while index < segments.len() {
        if index > 0 {
            #[cfg(creusot)]
            proof_assert! {
                joined_step(segments@, index@);
                joined(segments@.subsequence(0, index@ + 1)) == joined(segments@.subsequence(0, index@))
                    .concat(Seq::singleton('/'))
                    .concat(segments@[index@]@)
            };
            path.push('/');
        }
        path.push_str(segments[index]);
        index += 1;
    }
    #[cfg(creusot)]
    proof_assert! { segments@.subsequence(0, segments@.len()).ext_eq(segments@) };
    #[cfg(creusot)]
    proof_assert! { segments@.subsequence(0, segments@.len()) == segments@ };
    path
}

#[cfg(test)]
mod tests;
