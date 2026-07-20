use std::path::{Path, PathBuf};

/// Express `path` relative to `base`, mirroring Node's
/// `path.relative(base, path)`: the shortest relative path when the two
/// share a filesystem root, otherwise the absolute `path` (Node likewise
/// returns the absolute target when the inputs cannot be related).
///
/// On Windows the two `Prefix` components must match (drive letters
/// case-folded) before diffing; without that guard [`pathdiff::diff_paths`]
/// emits a re-anchored garbage path across drives or UNC shares.
#[must_use]
pub fn relative_path(base: &Path, path: &Path) -> PathBuf {
    relative_path_inner(base, path)
}

#[cfg(windows)]
fn relative_path_inner(base: &Path, path: &Path) -> PathBuf {
    let base = dunce::simplified(base);
    let path = dunce::simplified(path);
    if !same_path_root(path, base) {
        return path.to_path_buf();
    }
    pathdiff::diff_paths(path, base).unwrap_or_else(|| path.to_path_buf())
}

#[cfg(not(windows))]
fn relative_path_inner(base: &Path, path: &Path) -> PathBuf {
    pathdiff::diff_paths(path, base).unwrap_or_else(|| path.to_path_buf())
}

/// Whether `a` and `b` have an identical `Component::Prefix` after
/// `dunce::simplified`, with drive letters case-folded. UNC shares
/// only match when their server/share are written with identical
/// casing and variant — the check has to stay in lockstep with what
/// `pathdiff::diff_paths` will tolerate, since a variant-tolerant or
/// case-tolerant comparison here would let the downstream diff emit
/// a re-anchored garbage path on a `Prefix` mismatch it cannot relate.
#[cfg(windows)]
fn same_path_root(a: &Path, b: &Path) -> bool {
    fn first_prefix(path: &Path) -> Option<std::path::Prefix<'_>> {
        match path.components().next()? {
            std::path::Component::Prefix(p) => Some(p.kind()),
            _ => None,
        }
    }
    fn case_normalize(prefix: std::path::Prefix<'_>) -> std::path::Prefix<'_> {
        use std::path::Prefix::{Disk, VerbatimDisk};
        match prefix {
            Disk(d) => Disk(d.to_ascii_uppercase()),
            VerbatimDisk(d) => VerbatimDisk(d.to_ascii_uppercase()),
            other => other,
        }
    }
    match (first_prefix(a), first_prefix(b)) {
        (Some(pa), Some(pb)) => case_normalize(pa) == case_normalize(pb),
        (None, None) => true,
        _ => false,
    }
}
