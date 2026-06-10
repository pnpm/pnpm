//! Pacquet port of pnpm's
//! [`@pnpm/config.parse-overrides`](https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts).
//!
//! Splits each `pnpm.overrides` entry from `pnpm-workspace.yaml`
//! (or the root manifest's `pnpm.overrides`) into the structured shape
//! the resolver / read-package hook can act on:
//!
//! ```text
//! "foo"                 → target foo (any version)
//! "foo@2"               → target foo where parent dep declares ^2
//! "bar>foo"             → target foo only when nested under parent bar
//! "bar@1>foo@1"         → both halves narrowed by a semver range
//! "foo@3 || >=2"        → multi-range version constraint on the target
//! ```
//!
//! The `>` between parent and child is disambiguated from semver `>`/
//! `>=` operators by the upstream regex `/[^ |@]>/` — the byte before
//! the `>` must not be ` `, `|`, or `@`, so ranges like `>2` or
//! `3 || >=2` are not mistaken for a parent>child split.
//!
//! Catalog protocol: when the override value uses the `catalog:` form
//! it is resolved against the workspace's named catalogs before being
//! stored on the parsed entry, matching pnpm's
//! `parseOverrides(overrides, catalogs)` signature. A `catalog:` value
//! that misses (no entry, recursive, or uses a forbidden inner protocol)
//! surfaces as [`ParseOverridesError::CatalogInOverrides`] (matching
//! upstream's `ERR_PNPM_CATALOG_IN_OVERRIDES`). Pass an empty
//! [`Catalogs`] map when the caller has no catalogs configured — any
//! `catalog:` value will then fall through to the missing-entry branch,
//! same as upstream.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_resolver::{CatalogResolutionResult, WantedDependency, resolve_from_catalog};
use pacquet_catalogs_types::Catalogs;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use std::collections::HashMap;

/// A parsed `pnpm.overrides` entry. Mirrors upstream's
/// [`VersionOverride`](https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts#L8-L13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionOverride {
    /// The raw key from the overrides map, preserved verbatim so
    /// downstream code can surface it in diagnostics and key into the
    /// original config.
    pub selector: String,

    /// Set when the selector uses the `parent>child` form. `None` for
    /// "generic" overrides that apply regardless of the parent.
    pub parent_pkg: Option<PackageSelector>,

    /// The dependency the override targets.
    pub target_pkg: PackageSelector,

    /// The replacement spec (or `-` to delete, or a `link:`/`file:`
    /// reference). `catalog:` references are resolved against the
    /// workspace's catalogs before this is stored, so downstream
    /// consumers never see the `catalog:` form here.
    pub new_bare_specifier: String,
}

/// A name (and optional version-range scope) used to identify either
/// the parent or the target of a [`VersionOverride`]. Mirrors
/// upstream's
/// [`PackageSelector`](https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts#L15-L18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSelector {
    pub name: String,
    /// `Some("1")` for `foo@1`, `Some(">2")` for `foo@>2`, `None` for
    /// the bare `foo` shape (applies to any version).
    pub bare_specifier: Option<String>,
}

/// Failure modes of [`parse_overrides`] and [`parse_pkg_and_parent_selector`].
#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
pub enum ParseOverridesError {
    /// The selector half (either the parent or the target) doesn't
    /// resolve to a package name. Mirrors upstream's
    /// `ERR_PNPM_INVALID_SELECTOR` at
    /// <https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts#L65>.
    #[display(r#"Cannot parse the "{selector}" selector"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_SELECTOR))]
    InvalidSelector {
        #[error(not(source))]
        selector: String,
    },

    /// The override value uses the `catalog:` protocol but the
    /// configured catalogs can't resolve it (missing entry, recursive
    /// definition, or forbidden inner protocol). Mirrors upstream's
    /// `ERR_PNPM_CATALOG_IN_OVERRIDES` at
    /// <https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts#L35>.
    #[display("Could not resolve a catalog in the overrides: {message}")]
    #[diagnostic(code(ERR_PNPM_CATALOG_IN_OVERRIDES))]
    CatalogInOverrides {
        #[error(not(source))]
        message: String,
    },
}

/// Port of pnpm's
/// [`parseOverrides`](https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts#L20-L44).
///
/// Iteration follows the input map's iterator order. `HashMap` is
/// unordered, so the returned `Vec` is *not* guaranteed to match
/// upstream's `Object.entries` insertion order — for that, use
/// [`parse_overrides_iter`] with an ordered map (e.g. `IndexMap`)
/// or pre-sort by key. The functional behavior of each entry —
/// selector splitting via [`parse_pkg_and_parent_selector`] and
/// `catalog:` resolution — is independent of order.
pub fn parse_overrides(
    overrides: &HashMap<String, String>,
    catalogs: &Catalogs,
) -> Result<Vec<VersionOverride>, ParseOverridesError> {
    parse_overrides_iter(overrides.iter(), catalogs)
}

/// Stable-ordered variant of [`parse_overrides`] for callers that
/// drive their input through an ordered map (e.g. `IndexMap`) and want
/// the same ordering preserved on the output. Functionally identical
/// to [`parse_overrides`]; only the input iterator differs.
pub fn parse_overrides_iter<'a, Iter>(
    overrides: Iter,
    catalogs: &Catalogs,
) -> Result<Vec<VersionOverride>, ParseOverridesError>
where
    Iter: IntoIterator<Item = (&'a String, &'a String)>,
{
    let iter = overrides.into_iter();
    let (lower_bound, _) = iter.size_hint();
    let mut out = Vec::with_capacity(lower_bound);
    for (selector, new_bare_specifier) in iter {
        let (parent_pkg, target_pkg) = parse_pkg_and_parent_selector(selector)?;
        let resolved_specifier =
            resolve_catalog_in_value(catalogs, &target_pkg.name, new_bare_specifier)?;
        out.push(VersionOverride {
            selector: selector.clone(),
            parent_pkg,
            target_pkg,
            new_bare_specifier: resolved_specifier,
        });
    }
    Ok(out)
}

/// Flatten parsed overrides back into the `selector → newBareSpecifier`
/// map shape used by lockfile freshness checks. Mirrors upstream's
/// [`createOverridesMapFromParsed`](https://github.com/pnpm/pnpm/blob/4a36b9a110/lockfile/settings-checker/src/createOverridesMapFromParsed.ts).
/// The resulting map's values are post-catalog-resolution, so it
/// compares apples-to-apples against `lockfile.overrides`, which pnpm
/// writes out with `catalog:` already expanded.
#[must_use]
pub fn create_overrides_map_from_parsed(
    parsed_overrides: &[VersionOverride],
) -> HashMap<String, String> {
    parsed_overrides
        .iter()
        .map(|entry| (entry.selector.clone(), entry.new_bare_specifier.clone()))
        .collect()
}

/// Split a raw selector key into its (optional) parent half and its
/// target half. Mirrors upstream's
/// [`parsePkgAndParentSelector`](https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts#L46-L60).
pub fn parse_pkg_and_parent_selector(
    selector: &str,
) -> Result<(Option<PackageSelector>, PackageSelector), ParseOverridesError> {
    if let Some(delimiter_idx) = find_parent_delimiter(selector) {
        let parent_selector = &selector[..delimiter_idx];
        let child_selector = &selector[delimiter_idx + 1..];
        Ok((Some(parse_pkg_selector(parent_selector)?), parse_pkg_selector(child_selector)?))
    } else {
        Ok((None, parse_pkg_selector(selector)?))
    }
}

/// Position of the `>` byte that separates parent from child, when
/// the byte immediately before it is not ` `, `|`, or `@`. Returns
/// `None` for selectors that lack a `parent>child` form.
///
/// Mirrors upstream's `DELIMITER_REGEX = /[^ |@]>/` and the
/// `delimiterIndex++` adjustment that lands on the `>` itself.
fn find_parent_delimiter(selector: &str) -> Option<usize> {
    selector.as_bytes().windows(2).enumerate().find_map(|(idx, window)| {
        if matches!(window[0], b' ' | b'|' | b'@') {
            None
        } else if window[1] == b'>' {
            Some(idx + 1)
        } else {
            None
        }
    })
}

fn parse_pkg_selector(selector: &str) -> Result<PackageSelector, ParseOverridesError> {
    let wanted = parse_wanted_dependency(selector);
    let Some(name) = wanted.alias else {
        return Err(ParseOverridesError::InvalidSelector { selector: selector.to_string() });
    };
    Ok(PackageSelector { name, bare_specifier: wanted.bare_specifier })
}

/// Run the override value through the catalog resolver. Mirrors
/// upstream's `matchCatalogResolveResult` arm at
/// <https://github.com/pnpm/pnpm/blob/4a36b9a110/config/parse-overrides/src/index.ts#L28-L41>:
/// `found` returns the resolved specifier, `unused` (non-`catalog:`)
/// returns the value verbatim, and `misconfiguration` (missing entry
/// or recursive / forbidden inner protocol) raises
/// [`ParseOverridesError::CatalogInOverrides`] with the resolver's
/// error message.
fn resolve_catalog_in_value(
    catalogs: &Catalogs,
    target_name: &str,
    new_bare_specifier: &str,
) -> Result<String, ParseOverridesError> {
    let wanted = WantedDependency {
        alias: target_name.to_string(),
        bare_specifier: new_bare_specifier.to_string(),
    };
    match resolve_from_catalog(catalogs, &wanted) {
        CatalogResolutionResult::Found(found) => Ok(found.resolution.specifier),
        CatalogResolutionResult::Unused => Ok(new_bare_specifier.to_string()),
        CatalogResolutionResult::Misconfiguration(misconfiguration) => {
            Err(ParseOverridesError::CatalogInOverrides {
                message: misconfiguration.error.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests;
