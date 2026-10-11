//! Who owns the text a `tool:` / `runtime:` runtime declaration saves.

use pnpm_tool_protocol::strip_tool_protocol;

/// The version selector behind `tool:` / `runtime:` in a declaration or
/// request for `package_name`, when the node resolver owns the text it saves. `None` for a specifier of another
/// protocol, and for one naming another runtime.
///
/// `node`, `deno` and `bun` all reify through `tool:` / `runtime:`, but only the node
/// resolver moves the saved specifier onto the version it picked, through
/// `normalize_node_runtime_version_specifier`. The deno and bun resolvers
/// report the requested selector back unchanged, so `add` saves theirs
/// verbatim and an update has nothing of theirs to move.
pub(crate) fn node_runtime_version_spec<'a>(
    package_name: &str,
    specifier: &'a str,
) -> Option<&'a str> {
    if package_name != "node" {
        return None;
    }
    strip_tool_protocol(specifier)
}

#[cfg(test)]
mod tests;
