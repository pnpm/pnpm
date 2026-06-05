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
/// - Merges [`BUILTIN_NAMED_REGISTRIES`] with the user-supplied
///   `named_registries` (later wins on the same key — matches upstream's
///   spread semantics).
/// - Each prefix gets a trailing slash so a tarball URL under
///   `https://npm.pkg.github.com/@scope/pkg/-/pkg-1.0.0.tgz` matches
///   `https://npm.pkg.github.com/` but a sibling URL under
///   `https://npm.pkg.github.com-evil/...` does not.
/// - Output is sorted longest-first so two registries sharing a host
///   but differing by path (`https://npm/team-a/` vs
///   `https://npm/team-b/`) route to the deeper match.
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
/// `(name, tarball)` pair.
///
/// Mirrors upstream's
/// [`pickRegistryForVersion`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L591-L611):
///
/// 1. If the lockfile records a `tarball` URL **and** it starts with
///    one of the named-registry prefixes, return that prefix
///    (longest-match wins).
/// 2. Otherwise fall back to scope routing — `@scope/foo` consults
///    the `registries[@scope]` entry if present, else
///    `registries.default`. Ports upstream's
///    [`pickRegistryForPackage`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/pick-registry-for-package/src/index.ts#L3-L6).
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

/// Default-vs-scope routing for an npm package. Mirrors pnpm's
/// [`pickRegistryForPackage`](https://github.com/pnpm/pnpm/blob/main/config/pick-registry-for-package/src/index.ts).
///
/// Routing rules:
///
/// 1. **`npm:` alias.** When `bare_specifier` is an `npm:` alias the
///    *alias target* decides routing, not the local key:
///    - `npm:@scope/name@<spec>` → `registries[@scope]`.
///    - `npm:name@<spec>` (unscoped target) → `registries["default"]`,
///      never the local alias's scope, because the fetched package is
///      unscoped and doesn't live on a scoped registry.
/// 2. **Plain spec.** Falls back to `pkg_name`'s scope when present;
///    otherwise `registries["default"]`.
#[must_use]
pub fn pick_registry_for_package(
    registries: &HashMap<String, String>,
    pkg_name: &str,
    bare_specifier: Option<&str>,
) -> String {
    let scope = match bare_specifier.and_then(|spec| spec.strip_prefix("npm:")) {
        Some(target) => scope_of(target),
        None => scope_of(pkg_name),
    };
    if let Some(scope) = scope
        && let Some(url) = registries.get(scope)
    {
        return url.clone();
    }
    registries.get("default").cloned().unwrap_or_default()
}

fn scope_of(name: &str) -> Option<&str> {
    if !name.starts_with('@') {
        return None;
    }
    name.find('/').map(|sep| &name[..sep])
}

#[cfg(test)]
mod tests;
