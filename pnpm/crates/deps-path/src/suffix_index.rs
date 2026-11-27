use pnpm_pkg_name_split::groups::dep_path_suffix_start;

/// Index pair returned by [`index_of_dep_path_suffix`]. Both fields are
/// byte offsets into the original depPath, or `None` when the suffix is
/// absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepPathSuffixIndex {
    pub peers_index: Option<usize>,
    pub patch_hash_index: Option<usize>,
}

/// Walk `dep_path` from right to left, balancing parentheses, and find
/// the boundary of the peer-suffix and (optional) `(patch_hash=…)`
/// segments.
#[must_use]
pub fn index_of_dep_path_suffix(dep_path: &str) -> DepPathSuffixIndex {
    match dep_path_suffix_start(dep_path) {
        Some(start) => suffix_index_from_start(dep_path, start),
        None => DepPathSuffixIndex { peers_index: None, patch_hash_index: None },
    }
}

fn suffix_index_from_start(dep_path: &str, start: usize) -> DepPathSuffixIndex {
    if dep_path[start..].starts_with("(patch_hash=") {
        let peers_index = dep_path[start + 2..]
            .find('(')
            .map(|off| start + 2 + off);
        return DepPathSuffixIndex { peers_index, patch_hash_index: Some(start) };
    }
    DepPathSuffixIndex { peers_index: Some(start), patch_hash_index: None }
}

/// Strip the peer-suffix and `(patch_hash=…)` segments from `dep_path`,
/// returning just the `pkgId` (no patch hash) prefix.
#[must_use]
pub fn remove_suffix(dep_path: &str) -> &str {
    let DepPathSuffixIndex { peers_index, patch_hash_index } = index_of_dep_path_suffix(dep_path);
    if let Some(idx) = patch_hash_index {
        return &dep_path[..idx];
    }
    if let Some(idx) = peers_index {
        return &dep_path[..idx];
    }
    dep_path
}

/// Strip just the peer-suffix from `dep_path`, keeping the
/// `(patch_hash=…)` segment if present.
#[must_use]
pub fn get_pkg_id_with_patch_hash(dep_path: &str) -> &str {
    let DepPathSuffixIndex { peers_index, .. } = index_of_dep_path_suffix(dep_path);
    match peers_index {
        Some(idx) => &dep_path[..idx],
        None => dep_path,
    }
}

#[cfg(test)]
mod tests;
