use std::borrow::Cow;

use node_semver::Range;
use pnpm_lockfile::{PkgName, PkgNameVerPeer, Prefix, RegistryContext, VersionPart, same_registry};

/// Whether the version locked as `key` for the child `alias` still satisfies
/// `bare_specifier`, from the registry the specifier resolves against. A
/// plain range resolves against the registry `alias`'s scope routes, a
/// `<prefix>:` specifier against the prefix's registry, and the key names
/// its registry the same way, so a locked entry never satisfies a specifier
/// aimed at another registry, however either is spelled.
///
/// `None` when the reuse gate cannot judge the specifier: it is neither a
/// semver range nor a named-registry specifier, and the resolver resolves it
/// again.
#[must_use]
pub fn locked_child_admits(
    alias: &PkgName,
    key: &PkgNameVerPeer,
    bare_specifier: &str,
    registries: &RegistryContext,
) -> Option<bool> {
    let wanted = WantedRegistryRange::parse(alias, bare_specifier, registries)?;
    let locked_version = match key.suffix.registry_qualified() {
        Some((_, version)) => version,
        None => key.suffix.version_semver()?,
    };
    let locked_registry = registries.registry_of_key(key)?;
    Some(
        same_registry(locked_registry, wanted.registry)
            && key.name == *wanted.name
            && wanted.range.satisfies(locked_version),
    )
}

/// Whether `key` is the key a fresh resolve of `bare_specifier` for `alias`
/// would mint, so a reused subtree records the identity the configuration
/// writes today: a plain range names the prefix its registry declares, a
/// `<prefix>:` specifier the prefix that registry's keys carry (a declared
/// prefix, or a built-in alias as spelled). A specifier the gate cannot read
/// falls back to [`RegistryContext::rekeyed`]. A key that is not a registry
/// version (`file:`, `runtime:`, a URL) is left as it is.
#[must_use]
pub(crate) fn locked_key_is_current(
    alias: &PkgName,
    key: &PkgNameVerPeer,
    bare_specifier: &str,
    registries: &RegistryContext,
) -> bool {
    if key.suffix.prefix() != Prefix::None
        || !matches!(
            key.suffix.version(),
            VersionPart::Semver(_) | VersionPart::RegistryQualified { .. },
        )
    {
        return true;
    }
    match WantedRegistryRange::parse(alias, bare_specifier, registries) {
        Some(wanted) => {
            wanted.key_prefix == key.suffix.registry_qualified().map(|(prefix, _)| prefix)
        }
        None => registries.rekeyed(key).is_none(),
    }
}

/// A specifier the reuse gate can judge: the package it names, the registry
/// it resolves against, and the range it admits.
struct WantedRegistryRange<'a> {
    name: Cow<'a, PkgName>,
    registry: &'a str,
    /// The prefix a key minted for this specifier carries: the one the
    /// registry's keys carry, or a built-in alias as spelled.
    key_prefix: Option<&'a str>,
    range: Range,
}

impl<'a> WantedRegistryRange<'a> {
    /// `None` for a specifier that is neither a semver range nor a
    /// named-registry specifier (`<prefix>:[<name>@]<range>`) of a declared
    /// prefix.
    fn parse(
        alias: &'a PkgName,
        bare_specifier: &'a str,
        registries: &'a RegistryContext,
    ) -> Option<Self> {
        if let Some((prefix, body)) = bare_specifier.split_once(':')
            && let Some(registry) = registries.registry_of_prefix(prefix)
        {
            // `@scope/name@range` splits at the last `@`; a bare `range` has
            // none (or only the leading one of a scope, which never delimits
            // a version). A spec that names no package names the alias.
            let (name, range) = match body
                .rfind('@')
                .filter(|index| *index > 0)
            {
                Some(index) => {
                    (Cow::Owned(body[..index].parse::<PkgName>().ok()?), &body[index + 1..])
                }
                None => (Cow::Borrowed(alias), body),
            };
            return Some(Self {
                name,
                registry,
                key_prefix: registries.key_prefix_for_alias(prefix),
                range: range.parse().ok()?,
            });
        }
        let registry = registries.routed_registry(&alias.to_string());
        Some(Self {
            name: Cow::Borrowed(alias),
            registry,
            key_prefix: registries.key_prefix_for_registry(registry),
            range: bare_specifier.parse().ok()?,
        })
    }
}
