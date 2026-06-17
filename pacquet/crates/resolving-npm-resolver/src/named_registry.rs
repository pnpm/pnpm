//! Named-registry routing for the npm verifier.
//!
//! Lockfile entries carry a `tarball` URL recording where the
//! tarball was downloaded from. When that URL falls under a named
//! registry (`gh:` → `https://npm.pkg.github.com/`, custom user
//! mappings), the verifier must hit *that* registry's metadata
//! endpoint, not the scope-derived default — otherwise an entry
//! resolved via a named registry would 404 or, worse, hit a stale
//! mirror under the default registry.
//!
//! Ports the routing piece of upstream's
//! [`createNpmResolutionVerifier.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L118-L139)
//! plus the `BUILTIN_NAMED_REGISTRIES` constant from
//! [`parseBareSpecifier.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/parseBareSpecifier.ts#L87-L89).

use std::collections::HashMap;

use derive_more::{Display, Error};
use miette::Diagnostic;
pub use pacquet_lockfile::pick_registry_for_package;
use reqwest::Url;

/// Built-in named-registry aliases the resolver recognizes
/// out of the box. Mirrors upstream's `BUILTIN_NAMED_REGISTRIES`.
pub const BUILTIN_NAMED_REGISTRIES: &[(&str, &str)] = &[("gh", "https://npm.pkg.github.com/")];

/// Failure from [`merge_named_registries`]. Mirrors upstream's
/// [`ERR_PNPM_INVALID_NAMED_REGISTRY_URL`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/index.ts#L642-L656).
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
}

/// Merge user-supplied named-registry aliases on top of the built-in
/// defaults, validating each URL. User entries override the built-ins
/// on key collision (later wins, matching upstream's spread semantics)
/// so GHES users can point `gh` at an enterprise host.
///
/// Mirrors upstream's
/// [`mergeNamedRegistries`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/index.ts#L642-L656).
pub fn merge_named_registries(
    user_defined: &HashMap<String, String>,
) -> Result<HashMap<String, String>, MergeNamedRegistriesError> {
    let mut merged: HashMap<String, String> = BUILTIN_NAMED_REGISTRIES
        .iter()
        .map(|(name, url)| ((*name).to_string(), (*url).to_string()))
        .collect();
    for (alias, url) in user_defined {
        if !is_valid_http_url(url) {
            return Err(MergeNamedRegistriesError::InvalidUrl {
                alias: alias.clone(),
                url: url.clone(),
            });
        }
        merged.insert(alias.clone(), url.clone());
    }
    Ok(merged)
}

fn is_valid_http_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
}

/// Build the sorted-by-length list of registry URL prefixes the
/// verifier matches a tarball URL against.
///
/// Merges [`BUILTIN_NAMED_REGISTRIES`] with the user-supplied
/// `named_registries` (later wins on the same key — matches upstream's
/// spread semantics). Each prefix carries a trailing slash so prefix
/// matching can't be fooled by a same-host-different-suffix sibling,
/// and the output is sorted longest-first so the deepest matching
/// prefix wins.
#[must_use]
pub fn build_named_registry_prefixes(named_registries: &HashMap<String, String>) -> Vec<String> {
    let mut merged: HashMap<&str, String> = HashMap::new();
    for (name, url) in BUILTIN_NAMED_REGISTRIES {
        merged.insert(name, (*url).to_string());
    }
    for (name, url) in named_registries {
        // `to_string()` on `&String` triggers a clippy lint elsewhere
        // in the codebase; `clone` is the idiomatic equivalent.
        merged.insert(name.as_str(), url.clone());
    }

    let mut prefixes: Vec<String> = merged
        .into_values()
        .filter_map(|url| Url::parse(&url).ok())
        .map(|parsed| {
            let mut pathname = parsed.path().to_string();
            if !pathname.ends_with('/') {
                pathname.push('/');
            }
            format!("{}{}", parsed.origin().ascii_serialization(), pathname)
        })
        .collect();
    prefixes.sort_by_key(|prefix| std::cmp::Reverse(prefix.len()));
    prefixes
}

/// Pick the registry URL the verifier should hit for a given
/// `(name, tarball)` pair. A tarball URL under a named-registry prefix
/// routes to that registry; otherwise routing falls back to scope.
///
/// Mirrors upstream's
/// [`pickRegistryForVersion`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L591-L611),
/// falling back to upstream's
/// [`pickRegistryForPackage`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/pick-registry-for-package/src/index.ts#L3-L6).
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
