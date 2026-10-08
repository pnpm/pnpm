use super::reduce_named_registry_spec;
use node_semver::{Range, Version};
use pnpm_lockfile::{PkgNameVerPeer, Prefix, ResolvedDependencySpec};

/// Whether `bare_specifier` admits the version `spec` locked as `key`. `None`
/// when the locked version or the specifier has no comparable form.
pub(super) fn admits_locked_version(
    spec: &ResolvedDependencySpec,
    key: &PkgNameVerPeer,
    bare_specifier: &str,
) -> Option<bool> {
    let ver_peer = spec.version.ver_peer()?;
    if ver_peer.prefix() == Prefix::Runtime {
        return Some(runtime_selector_keeps_locked_version(
            &spec.specifier,
            bare_specifier,
            ver_peer.version_semver()?,
        ));
    }
    if let Some((registry_name, version)) = ver_peer.registry_qualified() {
        let range = reduce_named_registry_spec(registry_name, &key.name, bare_specifier)?
            .parse::<Range>()
            .ok()?;
        return Some(range.satisfies(version));
    }
    let range = bare_specifier.parse::<Range>().ok()?;
    Some(range.satisfies(ver_peer.version_semver()?))
}

/// Whether a `runtime:` dependency keeps its locked version: the selector is
/// unchanged, or it names the same release channel (`rc/^24` keeps only an
/// `rc/` pin) and its range admits `locked_version`.
fn runtime_selector_keeps_locked_version(
    locked_specifier: &str,
    bare_specifier: &str,
    locked_version: &Version,
) -> bool {
    if locked_specifier == bare_specifier {
        return true;
    }
    let (Some(locked), Some(wanted)) = (
        locked_specifier.strip_prefix(Prefix::Runtime.as_str()),
        bare_specifier.strip_prefix(Prefix::Runtime.as_str()),
    ) else {
        return false;
    };
    let (locked_channel, _) = split_release_channel(locked);
    let (wanted_channel, wanted_range) = split_release_channel(wanted);
    locked_channel == wanted_channel
        && wanted_range
            .parse::<Range>()
            .is_ok_and(|range| range.satisfies(locked_version))
}

fn split_release_channel(selector: &str) -> (Option<&str>, &str) {
    match selector.split_once('/') {
        Some((channel, range)) => (Some(channel), range),
        None => (None, selector),
    }
}
