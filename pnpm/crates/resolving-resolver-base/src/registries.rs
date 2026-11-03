use std::sync::Arc;

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
