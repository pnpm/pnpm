use pacquet_crypto_hash::shorten_virtual_store_name;

/// Turn a depPath into a filesystem-safe directory name.
#[must_use]
pub fn dep_path_to_filename(dep_path: &str, max_length_without_hash: usize) -> String {
    let mut filename = dep_path_to_filename_unescaped(dep_path);
    filename = filename.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|', '#'], "+");
    if filename.contains('(') {
        if filename.ends_with(')') {
            filename.pop();
        }
        filename = filename.replace(")(", "_").replace(['(', ')'], "_");
    }
    shorten_virtual_store_name(filename, max_length_without_hash)
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
    let Some(rel) = after_first.iter().position(|&b| b == b'@') else {
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
