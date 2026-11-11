use super::locked_child_admits;
use node_semver::{Range, Version};
use pnpm_lockfile::{PkgName, PkgNameVerPeer, Prefix, RegistryContext, ResolvedDependencySpec};
use pnpm_semver_include_prerelease::IncludePrereleaseRange;
use pnpm_tool_protocol::strip_tool_protocol;

/// Whether `bare_specifier` admits the version `spec` locked as `key` for
/// the importer dependency `alias`. `None` when the locked version or the
/// specifier has no comparable form.
pub(super) fn admits_locked_version(
    spec: &ResolvedDependencySpec,
    (alias, key): (&PkgName, &PkgNameVerPeer),
    bare_specifier: &str,
    registries: &RegistryContext,
) -> Option<bool> {
    let ver_peer = spec.version.ver_peer()?;
    if ver_peer.prefix() == Prefix::Runtime {
        return Some(runtime_selector_keeps_locked_version(
            &spec.specifier,
            bare_specifier,
            ver_peer.version_semver()?,
        ));
    }
    locked_child_admits(alias, key, bare_specifier, registries)
}

/// Whether the range of a `runtime:` dependency of a package admits the
/// runtime version locked as `key`. A snapshot records no specifier to compare
/// a selector against, so a selector without a range admits nothing. `None`
/// when `key` is not a runtime.
pub(super) fn runtime_child_admits(key: &PkgNameVerPeer, bare_specifier: &str) -> Option<bool> {
    if key.suffix.prefix() != Prefix::Runtime {
        return None;
    }
    let locked_version = key.suffix.version_semver()?;
    let admits = strip_tool_protocol(bare_specifier)
        .map(|selector| split_release_channel(selector).1)
        .and_then(runtime_range)
        .is_some_and(|range| range.satisfies(locked_version));
    Some(admits)
}

/// Whether a `tool:` / `runtime:` dependency keeps its locked version: a range selector
/// names the locked release channel (`rc/^24` keeps only an `rc/` pin) and
/// admits `locked_version`, and any other selector is unchanged.
fn runtime_selector_keeps_locked_version(
    locked_specifier: &str,
    bare_specifier: &str,
    locked_version: &Version,
) -> bool {
    let (Some(locked), Some(wanted)) =
        (strip_tool_protocol(locked_specifier), strip_tool_protocol(bare_specifier))
    else {
        return locked_specifier == bare_specifier;
    };
    let (locked_channel, _) = split_release_channel(locked);
    let (wanted_channel, wanted_range) = split_release_channel(wanted);
    match runtime_range(wanted_range) {
        Some(range) => locked_channel == wanted_channel && range.satisfies(locked_version),
        None => locked == wanted,
    }
}

/// A release channel such as `rc` or `nightly` publishes only prereleases,
/// so a runtime range matches them the way the runtime resolvers pick.
fn runtime_range(range: &str) -> Option<IncludePrereleaseRange> {
    range
        .parse::<Range>()
        .ok()
        .map(|_| IncludePrereleaseRange::parse(range))
}

fn split_release_channel(selector: &str) -> (Option<&str>, &str) {
    match selector.split_once('/') {
        Some((channel, range)) => (Some(channel), range),
        None => (None, selector),
    }
}
