/// `true` when `dep_path` is a runtime engine entry of the shape
/// `(node|bun|deno)@runtime:…`. Mirrors pnpm's
/// [`isRuntimeDepPath`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/src/index.ts#L215-L219)
/// — the only three runtimes pnpm v11 recognises as engine deps.
///
/// The check is byte-level (not parsed) so callers that hold the raw
/// snapshot key can filter without round-tripping through [`crate::DepPath`].
#[must_use]
pub fn is_runtime_dep_path(dep_path: &str) -> bool {
    for prefix in ["node@runtime:", "bun@runtime:", "deno@runtime:"] {
        if let Some(rest) = dep_path.strip_prefix(prefix)
            && !rest.is_empty()
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests;
