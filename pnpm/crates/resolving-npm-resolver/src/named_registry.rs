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
pub use pnpm_lockfile::pick_registry_for_package;
use reqwest::Url;

pub use pnpm_config::BUILTIN_REGISTRIES_BY_PREFIX;

/// Failure from [`merge_named_registries`], surfaced with the
/// `ERR_PNPM_INVALID_NAMED_REGISTRY_URL` code.
///
/// Surfaced at resolver construction so a malformed URL in the
/// user's `pnpm-workspace.yaml#registries` fails fast instead of
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
        help("Change the prefix on the corresponding registries entry.")
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
        help("Change the prefix on the corresponding registries entry.")
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
        if pnpm_deps_path::is_reserved_version_prefix(alias) {
            return Err(MergeNamedRegistriesError::ReservedAlias { alias: alias.clone() });
        }
        if !pnpm_deps_path::is_well_formed_registry_name(alias) {
            return Err(MergeNamedRegistriesError::MalformedAlias { alias: alias.clone() });
        }
        if !is_valid_http_url(url) {
            return Err(MergeNamedRegistriesError::InvalidUrl {
                alias: alias.clone(),
                url: url.clone(),
            });
        }
    }
    let mut merged: HashMap<String, String> = BUILTIN_REGISTRIES_BY_PREFIX
        .iter()
        .map(|(name, url)| ((*name).to_string(), (*url).to_string()))
        .collect();
    for (name, url) in user_defined {
        merged.insert(name.clone(), url.clone());
    }
    Ok(merged)
}

fn is_valid_http_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
}

/// Trailing slash so `https://npm.pkg.github.com-evil/` cannot match.
/// Equal lengths tie-break lexicographically, since length alone leaves
/// the order to `HashMap` iteration.
#[must_use]
pub fn named_registry_tarball_prefixes(
    registries_by_prefix: &HashMap<String, String>,
) -> Vec<String> {
    let mut prefixes: Vec<String> = registries_by_prefix
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
