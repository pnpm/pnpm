use crate::suffix_index::index_of_dep_path_suffix;

/// Extract the `pkgId` substring from a `dep_path`, mirroring pnpm's
/// [`tryGetPackageId`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/src/index.ts#L72-L88).
///
/// The function combines two transforms:
///
/// 1. Strip the peer-graph suffix and the `(patch_hash=…)` segment via
///    [`index_of_dep_path_suffix`] — same balanced-paren scan
///    [`crate::remove_suffix`] uses.
/// 2. When the trimmed result contains a `:`, drop the leading
///    `<name>@` prefix so the returned id is the bare resolution id
///    (a tarball URL, a git URL, ...). Pnpm keeps the prefix for
///    `runtime:` engine entries — those carry their name in the
///    pkgId by design.
///
/// The result is a borrowed slice of `dep_path` when neither transform
/// applies, or an owned [`String`] when the second transform (name
/// prefix strip) runs. Callers that always need ownership can
/// `.to_string()`.
#[must_use]
pub fn try_get_package_id(dep_path: &str) -> std::borrow::Cow<'_, str> {
    let suffix_index = index_of_dep_path_suffix(dep_path);
    let sep_index = suffix_index.patch_hash_index.or(suffix_index.peers_index);
    let trimmed = match sep_index {
        Some(idx) => &dep_path[..idx],
        None => dep_path,
    };
    if !trimmed.contains(':') {
        return std::borrow::Cow::Borrowed(trimmed);
    }
    // Drop the leading `<name>@` prefix. `indexOf('@', 1)` in pnpm
    // skips position 0 so a leading `@` on a scoped name doesn't
    // count as the separator.
    let Some(at_idx) = trimmed[1..].find('@').map(|off| off + 1) else {
        return std::borrow::Cow::Borrowed(trimmed);
    };
    let new_pkg_id = &trimmed[at_idx + 1..];
    if new_pkg_id.starts_with("runtime:") {
        return std::borrow::Cow::Borrowed(trimmed);
    }
    std::borrow::Cow::Owned(new_pkg_id.to_string())
}

#[cfg(test)]
mod tests;
