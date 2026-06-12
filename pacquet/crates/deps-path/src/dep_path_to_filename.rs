use pacquet_crypto_hash::shorten_virtual_store_name;

/// Turn a depPath into a filesystem-safe directory name. Mirrors pnpm's
/// [`depPathToFilename`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L169-L180).
///
/// Pipeline:
///
/// 1. **Escape the scheme prefix.** A `file:` prefix has its `:`
///    rewritten to `+`. The unescape branch upstream calls
///    `depPathToFilenameUnescaped`.
/// 2. **Strip a leading `/`** for the relative-depPath shape (legacy
///    pre-v9 lockfiles), then re-join the `@version` half so the
///    resulting name still has the `name@version` shape — upstream's
///    `${first}@${rest}` rebuild is a no-op for already-flat depPaths
///    and pacquet's port matches it directly.
/// 3. **Replace path-unsafe characters** (`\\ / : * ? " < > | #`) with
///    `+`.
/// 4. **Flatten parens** — strip the trailing `)`, then rewrite `)(`,
///    `(`, and `)` to `_`. After this step a depPath like
///    `foo@1.0.0(bar@2.0.0)` becomes `foo@1.0.0_bar@2.0.0`.
/// 5. **Cap length / case** via
///    [`pacquet_crypto_hash::shorten_virtual_store_name`]. Same trailing
///    branch the flat-name call sites already consume — single source of
///    truth for the truncation arithmetic and `file+` carve-out.
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
/// `/`, and re-join `@version`. Mirrors pnpm's private
/// [`depPathToFilenameUnescaped`](https://github.com/pnpm/pnpm/blob/097983fbca/deps/path/src/index.ts#L182-L192).
fn dep_path_to_filename_unescaped(dep_path: &str) -> String {
    if dep_path.starts_with("file:") {
        return dep_path.replacen(':', "+", 1);
    }
    let trimmed = dep_path.strip_prefix('/').unwrap_or(dep_path);
    // Upstream's `depPath.indexOf('@', 1)` skips position 0 so a leading
    // `@` on a scoped name (`@scope/foo`) doesn't get treated as the
    // version separator. The `len() < 2` guard mirrors that
    // out-of-range scan returning -1: nothing to rebuild, return the
    // string as-is. Without it the `[1..]` slice panics on empty /
    // single-byte input.
    if trimmed.len() < 2 {
        return trimmed.to_string();
    }
    let after_first = &trimmed.as_bytes()[1..];
    let Some(rel) = after_first.iter().position(|&b| b == b'@') else {
        return trimmed.to_string();
    };
    let split = rel + 1;
    let (name, rest) = trimmed.split_at(split);
    // Upstream rebuilds as `${name}@${rest.slice(1)}` — i.e. it consumes
    // the `@` and re-emits one. The transformation is a no-op for any
    // input whose `name` slot does not already end in `@`; matches the
    // upstream form byte-for-byte.
    let rest = rest.strip_prefix('@').unwrap_or(rest);
    format!("{name}@{rest}")
}

#[cfg(test)]
mod tests;
