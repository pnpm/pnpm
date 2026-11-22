//! Who owns the text a `tool:` / `runtime:` runtime declaration saves.

#[cfg(feature = "runtime-provisioning")]
use pnpm_engine_runtime_node_resolver::normalize_node_runtime_version_specifier;
use pnpm_package_manifest::is_runtime_alias;
#[cfg(feature = "runtime-provisioning")]
use pnpm_resolving_npm_resolver::infer_range_spec_style;
#[cfg(feature = "runtime-provisioning")]
use pnpm_resolving_resolver_base::parse_strict_semver_range;
use pnpm_tool_protocol::{
    RUNTIME_PROTOCOL,
    strip_tool_protocol,
};

/// The range a compatible update saves for a declared Node.js runtime.
#[cfg(feature = "runtime-provisioning")]
#[must_use]
pub fn updated_node_runtime_specifier(selector: &str, resolved: &node_semver::Version) -> String {
    let range = selector.split_once('/').map_or(selector, |(_, range)| range);
    if resolved.pre_release.is_empty()
        && infer_range_spec_style(range).is_none()
        && !range.is_empty()
        && parse_strict_semver_range(range).is_some_and(|range| range.satisfies(resolved))
    {
        return range.to_string();
    }
    normalize_node_runtime_version_specifier(selector, &resolved.to_string(), None)
}

/// The version selector behind `tool:` / `runtime:` in a declaration or
/// request for `package_name`, when the node resolver owns the text it saves.
/// `None` for a specifier of another protocol, and for one naming another
/// runtime.
///
/// `node`, `deno` and `bun` all reify through `tool:` / `runtime:`, but only the node
/// resolver moves the saved specifier onto the version it picked, through
/// `normalize_node_runtime_version_specifier`. The deno and bun resolvers
/// report the requested selector back unchanged, so `add` saves it as asked
/// under `runtime:` ([`runtime_save_specifier`]) and an update has nothing
/// of theirs to move.
pub(crate) fn node_runtime_version_spec<'a>(
    package_name: &str,
    specifier: &'a str,
) -> Option<&'a str> {
    if package_name != "node" {
        return None;
    }
    strip_tool_protocol(specifier)
}

/// The `runtime:` specifier a deno or bun request saves, whichever spelling
/// it used. `None` for any other package or protocol.
///
/// The declaration folds into `devEngines.runtime` / `engines.runtime`, which
/// stores no protocol and reads back as `runtime:`, so the lockfile has to
/// record that spelling to stay readable by earlier pnpm versions.
pub(crate) fn runtime_save_specifier(package_name: &str, specifier: &str) -> Option<String> {
    if !is_runtime_alias(package_name) {
        return None;
    }
    strip_tool_protocol(specifier).map(|selector| format!("{RUNTIME_PROTOCOL}{selector}"))
}

#[cfg(test)]
mod tests;
