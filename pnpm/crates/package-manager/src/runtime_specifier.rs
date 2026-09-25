//! The `runtime:` protocol a `devEngines.runtime` / `engines.runtime` entry is
//! reified through, and who owns the text one saves.

/// The protocol a reified `devEngines.runtime` / `engines.runtime` entry is
/// declared through.
pub(crate) const RUNTIME_PROTOCOL: &str = "runtime:";

/// The version selector behind `runtime:` in a declaration or request for
/// `package_name`, when the node resolver owns the text it saves. `None` for a
/// specifier of another protocol, and for one naming another runtime.
///
/// `node`, `deno` and `bun` all reify through `runtime:`, but only the node
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
    specifier.strip_prefix(RUNTIME_PROTOCOL)
}

#[cfg(test)]
mod tests;
