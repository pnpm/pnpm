//! The `tool:` protocol: `<name>@tool:<selector>` names a tool pnpm
//! provisions itself (Node.js, Bun, Deno, Yarn 6) rather than an npm package.
//!
//! `runtime:` is accepted as an alias wherever a user writes a specifier.
//! Every specifier and lockfile key pnpm writes keeps `runtime:` until the
//! next lockfile major, because `engines.runtime` / `devEngines.runtime`
//! store no protocol and read back as `runtime:`.

/// The protocol of a provisioned tool.
pub const TOOL_PROTOCOL: &str = "tool:";

/// The alias of [`TOOL_PROTOCOL`], which every specifier and lockfile key
/// pnpm writes uses.
pub const RUNTIME_PROTOCOL: &str = "runtime:";

/// The version selector behind `tool:` or `runtime:`. `None` for any other
/// protocol.
#[must_use]
pub fn strip_tool_protocol(specifier: &str) -> Option<&str> {
    specifier
        .strip_prefix(TOOL_PROTOCOL)
        .or_else(|| specifier.strip_prefix(RUNTIME_PROTOCOL))
}

/// `true` when `specifier` is written in `tool:` or `runtime:`.
#[must_use]
pub fn is_tool_specifier(specifier: &str) -> bool {
    strip_tool_protocol(specifier).is_some()
}

/// `true` when `left` and `right` are the same tool request, treating
/// `tool:<selector>` and `runtime:<selector>` as equal.
#[must_use]
pub fn tool_specifiers_equal(left: &str, right: &str) -> bool {
    matches!(
        (strip_tool_protocol(left), strip_tool_protocol(right)),
        (Some(left), Some(right)) if left == right,
    )
}

#[cfg(test)]
mod tests;
