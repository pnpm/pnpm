//! Named-registry routing for the npm verifier.
//!
//! Lockfile entries carry a `tarball` URL recording where the
//! tarball was downloaded from. When that URL falls under a named
//! registry (`gh:` → `https://npm.pkg.github.com/`, custom user
//! mappings), the verifier must hit *that* registry's metadata
//! endpoint, not the scope-derived default — otherwise an entry
//! resolved via a named registry would 404 or, worse, hit a stale
//! mirror under the default registry.

use std::collections::HashMap;

use derive_more::{Display, Error};
use miette::Diagnostic;
pub use pacquet_lockfile::pick_registry_for_package;
use reqwest::Url;

/// Built-in named-registry aliases the resolver recognizes
/// out of the box.
///
/// `npmjs` is here so a dependency can be pinned to the public
/// registry even when `registry` points somewhere else, such as an
/// internal proxy. The `npm` prefix cannot serve that purpose: it is
/// reserved for the alias protocol (`npm:<name>@<range>`), which
/// resolves through the default registry.
///
/// These URLs are also the prefixes
/// [`KnownRegistries::tarball_prefixes`] matches a recorded tarball URL
/// against, so an org that proxies
/// npmjs should point `npmjs` at their proxy to keep verification
/// going there rather than to the public host.
pub const BUILTIN_NAMED_REGISTRIES: &[(&str, &str)] =
    &[("gh", "https://npm.pkg.github.com/"), ("npmjs", "https://registry.npmjs.org/")];

/// Failure from [`merge_named_registries`], surfaced with the
/// `ERR_PNPM_INVALID_NAMED_REGISTRY_URL` code.
///
/// Surfaced at resolver construction so a malformed URL in the
/// user's `pnpm-workspace.yaml#namedRegistries` fails fast instead of
/// turning into a confusing 404 during resolution.
#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeNamedRegistriesError {
    #[display(
        "The named registry alias '{alias}' is mapped to '{url}', which is not a valid http(s) URL."
    )]
    #[diagnostic(
        code(ERR_PNPM_INVALID_NAMED_REGISTRY_URL),
        help(
            "Provide a URL that starts with http:// or https://, e.g. https://npm.pkg.example.com/"
        )
    )]
    InvalidUrl {
        #[error(not(source))]
        alias: String,
        url: String,
    },
    #[display(
        "'{alias}' cannot be used as a named registry alias: it is a reserved dependency specifier prefix."
    )]
    #[diagnostic(
        code(ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME),
        help("Rename the entry in the namedRegistries setting.")
    )]
    ReservedAlias {
        #[error(not(source))]
        alias: String,
    },
    #[display(
        "'{alias}' cannot be used as a named registry alias: aliases must start with a letter and contain only letters, digits, \".\", \"_\", and \"-\"."
    )]
    #[diagnostic(
        code(ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME),
        help("Rename the entry in the namedRegistries setting.")
    )]
    MalformedAlias {
        #[error(not(source))]
        alias: String,
    },
}

/// Merge user-supplied named-registry aliases on top of the built-in
/// defaults, validating each URL. User entries override the built-ins
/// on key collision (later wins) so GHES users can point `gh` at an
/// enterprise host.
pub fn merge_named_registries(
    user_defined: &HashMap<String, String>,
) -> Result<HashMap<String, String>, MergeNamedRegistriesError> {
    for (alias, url) in user_defined {
        if pacquet_deps_path::is_reserved_version_prefix(alias) {
            return Err(MergeNamedRegistriesError::ReservedAlias { alias: alias.clone() });
        }
        if !pacquet_deps_path::is_well_formed_registry_name(alias) {
            return Err(MergeNamedRegistriesError::MalformedAlias { alias: alias.clone() });
        }
        if !is_valid_http_url(url) {
            return Err(MergeNamedRegistriesError::InvalidUrl {
                alias: alias.clone(),
                url: url.clone(),
            });
        }
    }
    Ok(KnownRegistries::new(user_defined).into_by_alias())
}

fn is_valid_http_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
}

/// Every registry pnpm can route to by alias, and the two ways that
/// set is consulted.
///
/// Built once from [`BUILTIN_NAMED_REGISTRIES`] plus the user's
/// `namedRegistries` (user wins on collision, so a GHES user can point
/// `gh` at their enterprise host). Holding both views together is the
/// point: a change to either input cannot alter one consumer without
/// the other.
#[derive(Debug, Clone)]
pub struct KnownRegistries {
    by_alias: HashMap<String, String>,
    tarball_prefixes: Vec<String>,
}

impl KnownRegistries {
    /// Merge the built-in aliases with the user's `namedRegistries`, user
    /// winning on collision, and derive both views.
    ///
    /// This is the only place the two are combined. [`merge_named_registries`]
    /// validates the user's entries and then delegates here, so a caller that
    /// has already validated can take the merge infallibly.
    #[must_use]
    pub fn new(named_registries: &HashMap<String, String>) -> Self {
        let mut by_alias: HashMap<String, String> = BUILTIN_NAMED_REGISTRIES
            .iter()
            .map(|(name, url)| ((*name).to_string(), (*url).to_string()))
            .collect();
        for (alias, url) in named_registries {
            by_alias.insert(alias.clone(), url.clone());
        }
        let tarball_prefixes = build_tarball_prefixes(&by_alias);
        Self { by_alias, tarball_prefixes }
    }

    /// Alias to registry URL, for an entry that names its registry in
    /// the dep path.
    #[must_use]
    pub fn by_alias(&self) -> &HashMap<String, String> {
        &self.by_alias
    }

    /// The URL prefixes a recorded tarball URL is matched against to
    /// decide which registry to verify an entry with, longest first so
    /// the deepest match wins.
    ///
    /// This is why adding an entry to [`BUILTIN_NAMED_REGISTRIES`] is
    /// not a local change: it also decides where verification traffic
    /// goes for lockfile entries that name no alias at all.
    #[must_use]
    pub fn tarball_prefixes(&self) -> &[String] {
        &self.tarball_prefixes
    }

    #[must_use]
    pub fn into_by_alias(self) -> HashMap<String, String> {
        self.by_alias
    }
}

/// Each prefix carries a trailing slash so matching can't be fooled by
/// a same-host-different-suffix sibling, and the output is sorted
/// longest-first so the deepest matching prefix wins. A URL that does
/// not parse is dropped rather than poisoning the list.
///
/// Equal-length prefixes tie-break lexicographically: length alone
/// leaves their relative order to `HashMap` iteration, which differs
/// between runs and makes the list — and anything asserting on it —
/// unstable.
fn build_tarball_prefixes(by_alias: &HashMap<String, String>) -> Vec<String> {
    let mut prefixes: Vec<String> = by_alias
        .values()
        .filter_map(|url| Url::parse(url).ok())
        .map(|parsed| {
            let mut pathname = parsed.path().to_string();
            if !pathname.ends_with('/') {
                pathname.push('/');
            }
            format!("{}{}", parsed.origin().ascii_serialization(), pathname)
        })
        .collect();
    prefixes.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    prefixes
}

/// Pick the registry URL the verifier should hit for a given
/// `(name, tarball)` pair. A tarball URL under a named-registry prefix
/// routes to that registry; otherwise routing falls back to scope via
/// [`pick_registry_for_package`].
#[must_use]
pub fn pick_registry_for_version(
    registries: &HashMap<String, String>,
    named_registry_prefixes: &[String],
    name: &str,
    tarball_url: Option<&str>,
) -> String {
    if let Some(url) = tarball_url
        && let Ok(parsed) = Url::parse(url)
    {
        // Normalize to the absolute URL string the prefix list is built from.
        let normalized = parsed.as_str();
        for prefix in named_registry_prefixes {
            if normalized.starts_with(prefix) {
                return prefix.clone();
            }
        }
    }
    pick_registry_for_package(registries, name, None)
}

#[cfg(test)]
mod tests;
