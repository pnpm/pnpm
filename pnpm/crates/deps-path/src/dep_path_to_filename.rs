use pnpm_crypto_hash::{hash_suffix_virtual_store_name, shorten_virtual_store_name};

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
    let escapes_ambiguously = has_ambiguous_escape(dep_path);
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
/// unambiguously.
fn has_ambiguous_escape(dep_path: &str) -> bool {
    let pkg_id = dep_path
        .split('(')
        .next()
        .unwrap_or_default();
    let pkg_id = pkg_id.strip_prefix('/').unwrap_or(pkg_id);
    let version = if pkg_id.starts_with("file:") {
        pkg_id
    } else {
        let Some((separator, _)) = pkg_id
            .match_indices('@')
            .find(|(index, _)| *index > 0)
        else {
            return false;
        };
        &pkg_id[separator + 1..]
    };
    let Some((_, location)) = version.split_once(':') else { return false };
    if crate::parse_registry_qualified_version(version).is_some() {
        return false;
    }
    let location = location.strip_prefix("//").unwrap_or(location);
    location.contains(['+', '\\', ':', '*', '?', '"', '<', '>', '|', '#'])
}

/// Pre-escape pass: rewrite `file:` to `file+`, strip a single leading
/// `/`, and re-join `@version`.
fn dep_path_to_filename_unescaped(dep_path: &str) -> String {
    if dep_path.starts_with("file:") {
        return dep_path.replacen(':', "+", 1);
    }
    let trimmed = dep_path.strip_prefix('/').unwrap_or(dep_path);
    // Scan for the `@` from position 1 so a leading `@` on a scoped name
    // (`@scope/foo`) doesn't get treated as the version separator. The
    // `len() < 2` guard handles the case where there's nothing to
    // rebuild: return the string as-is. Without it the `[1..]` slice
    // panics on empty / single-byte input.
    if trimmed.len() < 2 {
        return trimmed.to_string();
    }
    let after_first = &trimmed.as_bytes()[1..];
    let Some(rel) = after_first
        .iter()
        .position(|&b| b == b'@')
    else {
        return trimmed.to_string();
    };
    let split = rel + 1;
    let (name, rest) = trimmed.split_at(split);
    // Rebuild as `${name}@${rest[1..]}` — i.e. consume the `@` and
    // re-emit one. The transformation is a no-op for any input whose
    // `name` slot does not already end in `@`.
    let rest = rest.strip_prefix('@').unwrap_or(rest);
    format!("{name}@{rest}")
}

#[cfg(test)]
mod tests;
