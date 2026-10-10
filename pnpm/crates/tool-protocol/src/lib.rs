//! The `tool:` protocol: `<name>@tool:<selector>` names a tool pnpm
//! provisions itself (Node.js, Bun, Deno, Yarn 6) rather than an npm package.
//!
//! `runtime:` is accepted as an alias wherever a user writes a specifier.
//! Lockfile keys and the specifiers pnpm generates from `engines.runtime` /
//! `devEngines.runtime` keep `runtime:` until the next lockfile major.

/// The protocol of a provisioned tool.
pub const TOOL_PROTOCOL: &str = "tool:";

/// The alias of [`TOOL_PROTOCOL`], which lockfile keys use.
pub const RUNTIME_PROTOCOL: &str = "runtime:";

/// Splits `specifier` into the protocol as written (`tool:` or `runtime:`)
/// and the version selector behind it. `None` for any other protocol.
#[must_use]
pub fn split_tool_protocol(specifier: &str) -> Option<(&str, &str)> {
    [TOOL_PROTOCOL, RUNTIME_PROTOCOL]
        .into_iter()
        .find_map(|protocol| {
            specifier
                .strip_prefix(protocol)
                .map(|selector| (&specifier[..protocol.len()], selector))
        })
}

/// The version selector behind `tool:` or `runtime:`. `None` for any other
/// protocol.
#[must_use]
pub fn strip_tool_protocol(specifier: &str) -> Option<&str> {
    split_tool_protocol(specifier).map(|(_, selector)| selector)
}

/// `true` when `specifier` is written in `tool:` or `runtime:`.
#[must_use]
pub fn is_tool_specifier(specifier: &str) -> bool {
    split_tool_protocol(specifier).is_some()
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
