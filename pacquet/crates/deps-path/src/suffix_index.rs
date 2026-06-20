/// Index pair returned by [`index_of_dep_path_suffix`]. Both fields are
/// byte offsets into the original depPath, or `None` when the suffix is
/// absent. Mirrors pnpm's
/// [`indexOfDepPathSuffix`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L9-L31)
/// `{ peersIndex, patchHashIndex }` return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepPathSuffixIndex {
    pub peers_index: Option<usize>,
    pub patch_hash_index: Option<usize>,
}

/// Walk `dep_path` from right to left, balancing parentheses, and find
/// the boundary of the peer-suffix and (optional) `(patch_hash=…)`
/// segments. Mirrors pnpm's
/// [`indexOfDepPathSuffix`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L9-L31).
#[must_use]
pub fn index_of_dep_path_suffix(dep_path: &str) -> DepPathSuffixIndex {
    let bytes = dep_path.as_bytes();
    let absent = DepPathSuffixIndex { peers_index: None, patch_hash_index: None };
    if !dep_path.ends_with(')') {
        return absent;
    }

    let mut open: i32 = 1;
    // Scan from second-to-last byte down to byte 0. Upstream's loop
    // starts at `length - 2` and stops at `>= 0`; we mirror it byte-for-
    // byte (depPath is ASCII outside of the package-name slot, and pnpm
    // doesn't permit non-ASCII there either).
    let mut cursor = bytes.len().checked_sub(2);
    while let Some(idx) = cursor {
        match bytes[idx] {
            b'(' => open -= 1,
            b')' => open += 1,
            _ if open == 0 => {
                let start = idx + 1;
                if dep_path[start..].starts_with("(patch_hash=") {
                    let peers_index = dep_path[start + 2..].find('(').map(|off| start + 2 + off);
                    return DepPathSuffixIndex { peers_index, patch_hash_index: Some(start) };
                }
                return DepPathSuffixIndex { peers_index: Some(start), patch_hash_index: None };
            }
            _ => {}
        }
        cursor = idx.checked_sub(1);
    }
    absent
}

/// Strip the peer-suffix and `(patch_hash=…)` segments from `dep_path`,
/// returning just the `pkgId` (no patch hash) prefix. Mirrors pnpm's
/// [`removeSuffix`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L52-L61).
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
/// `(patch_hash=…)` segment if present. Mirrors pnpm's
/// [`getPkgIdWithPatchHash`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L63-L70).
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
