/// `true` when `dep_path` is a runtime engine entry of the shape
/// `(node|bun|deno)@runtime:…` — the only three runtimes recognised as
/// engine deps.
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
