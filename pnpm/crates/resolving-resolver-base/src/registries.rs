use pnpm_lockfile::RegistryServerType;
use serde::Deserialize;
use std::{collections::BTreeMap, fmt, num::NonZeroUsize, sync::Arc};

/// The registry the built-in `@jsr` scope routes to when the user has not
/// pointed it elsewhere.
pub const DEFAULT_JSR_REGISTRY: &str = "https://npm.jsr.io/";

/// Built-in named-registry aliases the resolver recognizes
/// out of the box.
///
/// `npmjs` is here so a dependency can be pinned to the public
/// registry even when `registry` points somewhere else, such as an
/// internal proxy. The `npm` prefix cannot serve that purpose: it is
/// reserved for the alias protocol (`npm:<name>@<range>`), which
/// resolves through the default registry.
///
/// These URLs are also the prefixes the npm verifier's
/// `named_registry_tarball_prefixes` matches a recorded tarball URL
/// against, so an org that proxies
/// npmjs should point `npmjs` at their proxy to keep verification
/// going there rather than to the public host.
pub const BUILTIN_REGISTRIES_BY_PREFIX: &[(&str, &str)] =
    &[("gh", "https://npm.pkg.github.com/"), ("npmjs", "https://registry.npmjs.org/")];

/// Whether the resolution has to read full packument metadata from a given
/// registry (and whether the dependency is optional).
pub type NeedsFullMetadataFn = dyn Fn(&str, bool) -> bool + Send + Sync;

/// Shared handle to a [`NeedsFullMetadataFn`].
pub type NeedsFullMetadataFor = Arc<NeedsFullMetadataFn>;

/// The package ecosystem whose packages a registry serves.
///
/// Closed, and narrower than the set of surfaces a pnpr server can host:
/// pnpm installs from these, so an ecosystem it cannot install from
/// has no meaning in a `registries` entry and is refused where the entry is
/// parsed rather than somewhere further in.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    #[default]
    Npm,
    Cargo,
    Pypi,
}

impl Ecosystem {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Ecosystem::Npm => "npm",
            Ecosystem::Cargo => "cargo",
            Ecosystem::Pypi => "pypi",
        }
    }
}

impl fmt::Display for Ecosystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Everything a project declares about one registry.
///
/// Unknown fields are captured rather than refused by serde on purpose, for
/// the same reason the configuration validates after parsing: a parse error renders
/// the offending source line verbatim, so refusing an `_authToken` field at
/// parse time would print the very credential being refused.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryDeclaration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_type: Option<RegistryServerType>,
    /// See [`pnpm_lockfile::RegistryOptions::supports_time_field`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_time_field: Option<bool>,
    /// The most requests pnpm keeps in flight to this registry's origin, within
    /// the overall `networkConcurrency`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_concurrency: Option<NonZeroUsize>,
    /// The scopes routed here, `@`-prefixed. A bare `@` is the scope-less
    /// default registry, the one the `registry` setting names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    #[serde(flatten)]
    pub prefixes: RegistryPrefixes,
    /// Which ecosystem's packages this registry serves. Absent is
    /// [`Ecosystem::Npm`], so every entry written before there was anything
    /// else to serve means what it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ecosystem: Option<Ecosystem>,
    /// Python package names or trailing-prefix patterns routed exclusively here.
    /// Omitted, or `*`, declares the default Python index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packages: Option<Vec<String>>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, serde_json::Value>,
}

impl Eq for RegistryDeclaration {}

/// The bare-specifier prefixes that address a registry.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
pub struct RegistryPrefixes {
    /// The bare-specifier prefix this registry answers to, as in
    /// `"foo": "work:^1.0.0"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// The other prefixes routed to this registry. A pnpr request carries
    /// them, `pnpm-workspace.yaml` refuses them: the request names every
    /// prefix the client resolves, while `prefix` holds the smallest, the one
    /// lockfile keys carry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

impl RegistryPrefixes {
    /// `prefix` followed by `aliases`.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.prefix
            .as_deref()
            .into_iter()
            .chain(self.aliases.iter().map(String::as_str))
    }
}

impl RegistryDeclaration {
    /// The ecosystem this entry serves, with the absent field read as
    /// [`Ecosystem::Npm`].
    #[must_use]
    pub fn ecosystem(&self) -> Ecosystem {
        self.ecosystem.unwrap_or_default()
    }
}
