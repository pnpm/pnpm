use pnpm_crypto_hash::{
    hash_suffix_virtual_store_name,
    shorten_virtual_store_name,
};

/// Turn a depPath into a filesystem-safe directory name.
#[must_use]
pub fn dep_path_to_filename(dep_path: &str, max_length_without_hash: usize) -> String {
    let mut filename = dep_path_to_filename_unescaped(dep_path);
    filename = filename.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|', '#'], "+");
    if filename.contains('(') {
        if filename.ends_with(')') {
            filename.pop();
        }
        filename = filename
            .replace(")(", "_")
            .replace(['(', ')'], "_");
    }
    // Windows strips trailing dots and spaces from path segments. Hashing
    // the unescaped name keeps it apart from a literal `+` path.
    let kept = filename
        .trim_end_matches(['.', ' '])
        .len();
    let trailing = filename.len() - kept;
    let escapes_ambiguously = has_ambiguous_escape(dep_path, &filename);
    if trailing > 0 {
        let mut escaped = filename[..kept].to_string();
        escaped.extend(std::iter::repeat_n('+', trailing));
        let hash_input = if escapes_ambiguously { dep_path } else { &filename };
        return hash_suffix_virtual_store_name(&escaped, hash_input, max_length_without_hash);
    }
    if escapes_ambiguously {
        return hash_suffix_virtual_store_name(&filename, dep_path, max_length_without_hash);
    }
    shorten_virtual_store_name(filename, max_length_without_hash)
}

/// Whether escaping the URL or path of a non-registry dependency could map
/// two distinct dep paths to one name. Only `/` after the scheme escapes
/// unambiguously. A peer suffix or parentheses in a URL escape to `_`, as
/// does a literal `_`. A name that already ends in a hash suffix could
/// equal the hashed name of another dep path.
fn has_ambiguous_escape(dep_path: &str, filename: &str) -> bool {
    let pkg_id = crate::remove_suffix(dep_path);
    let has_suffix = pkg_id.len() < dep_path.len();
    let pkg_id = pkg_id.strip_prefix('/').unwrap_or(pkg_id);
    let version = if pkg_id.starts_with("file:") {
        pkg_id
    } else {
        let Some(separator) = crate::version_separator_index(pkg_id) else {
            return false;
        };
        &pkg_id[separator + 1..]
    };
    let Some((_, location)) = version.split_once(':') else { return false };
    if crate::parse_registry_qualified_version(version).is_some() {
        return false;
    }
    if ends_with_hash_suffix(filename) || location.contains(['(', ')']) {
        return true;
    }
    // A local path is chosen by the project, so its peer suffix keeps the
    // unhashed name.
    if has_suffix && !version.starts_with("file:") {
        return true;
    }
    let location = location.strip_prefix("//").unwrap_or(location);
    location.contains(['+', '\\', ':', '*', '?', '"', '<', '>', '|', '#'])
}

/// Whether `filename` has the `_<hash>` shape that
/// [`hash_suffix_virtual_store_name`] appends.
fn ends_with_hash_suffix(filename: &str) -> bool {
    let Some(split) = filename.len().checked_sub(33) else { return false };
    let Some(suffix) = filename.get(split..) else { return false };
    let Some(hash) = suffix.strip_prefix('_') else { return false };
    hash.bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Pre-escape pass: rewrite `file:` to `file+`, strip a single leading
/// `/`, and re-join `@version`.
fn dep_path_to_filename_unescaped(dep_path: &str) -> String {
    if dep_path.starts_with("file:") {
        return dep_path.replacen(':', "+", 1);
    }
    let trimmed = dep_path.strip_prefix('/').unwrap_or(dep_path);
    let Some(split) = crate::version_separator_index(trimmed) else {
        return trimmed.to_string();
    };
    let (name, rest) = trimmed.split_at(split);
    let rest = rest.strip_prefix('@').unwrap_or(rest);
    format!("{name}@{rest}")
}

#[cfg(test)]
mod tests;
