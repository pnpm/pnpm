//! Pure version-picking logic over an already-fetched packument.
//!
//! Three call sites converge on this module:
//!
//! - [`pick_package_from_meta`] — given a parsed
//!   [`RegistryPackageSpec`] and a [`Package`] packument, pick the
//!   single [`PackageVersion`] that wins (or `Ok(None)` when no
//!   version satisfies). Applies the `minimumReleaseAge` filter
//!   (`publishedBy`) ahead of the per-spec branch.
//! - [`pick_version_by_version_range`] /
//!   [`pick_lowest_version_by_version_range`] — choose the
//!   highest/lowest version in `meta.versions` satisfying a range
//!   string, biased by an optional [`VersionSelectors`] preference
//!   table. The high-side variant also runs the deprecated-version
//!   fallback (if the max pick is deprecated and other versions
//!   exist, retry against the non-deprecated subset).
//! - [`filter_pkg_metadata_by_publish_date`] — derive a packument
//!   that contains only versions published at or before a cutoff,
//!   plus rewritten `dist-tags` pointing to the highest within-cutoff
//!   version per tag. Implements the `minimumReleaseAge` policy.
//!
//! The "pure picker" piece sits below the cache+fetch orchestration
//! in [`crate::pick_package()`]; both depend on this module but this
//! module pulls in no I/O.

pub(crate) use preferred_versions::dominant_lockfile_version;

pub(crate) use release_age::{PublishedByView, apply_published_by_policy};

pub use release_age::{filter_pkg_metadata_by_publish_date, filter_pkg_metadata_versions};

mod semver_range;
use semver_range::{max_satisfying, min_satisfying, semver_satisfies_loose};

mod preferred_versions;
use preferred_versions::prioritize_preferred_versions;

mod release_age;

use std::{
    cell::OnceCell,
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

use dashmap::DashMap;
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use pnpm_config::version_policy::{PackageVersionPolicy, PolicyMatch};
use pnpm_registry::{DerivedPackuments, Package, PackageVersion, PackageVersions};
use pnpm_resolving_resolver_base::{
    EXISTING_VERSION_SELECTOR_WEIGHT, VersionSelectorEntry, VersionSelectorType, VersionSelectors,
    parse_packument_timestamp,
};

/// Discriminator for [`RegistryPackageSpec::spec_type`]: the
/// `tag` / `version` / `range` triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryPackageSpecType {
    /// Exact version pin, e.g. `1.2.3`.
    Version,
    /// Dist-tag, e.g. `latest`, `next`.
    Tag,
    /// Semver range, e.g. `^1.0.0`.
    Range,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryRevisionSelector {
    Valid(u64),
    Invalid(String),
}

/// Parsed registry spec produced by the bare-specifier parser. The
/// picker (and the cache+fetch wrapper above it) consume this shape;
/// the parser that produces it lives in its own module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryPackageSpec {
    pub name: String,
    pub fetch_spec: String,
    pub spec_type: RegistryPackageSpecType,
    pub revision: Option<RegistryRevisionSelector>,
    /// Echo of the original bare specifier when the spec came from a
    /// tarball-URL parse. The resolver writes this back into
    /// `ResolveResult.normalized_bare_specifier`; the picker itself
    /// doesn't read it.
    pub normalized_bare_specifier: Option<String>,
}

impl RegistryPackageSpec {
    /// The `latest` dist-tag spec for `name`.
    pub fn latest_tag(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fetch_spec: "latest".to_string(),
            spec_type: RegistryPackageSpecType::Tag,
            revision: None,
            normalized_bare_specifier: None,
        }
    }
}

/// Options bundle for [`pick_package_from_meta`].
#[derive(Debug, Default)]
pub struct PickPackageFromMetaOptions<'a> {
    /// Per-importer hints biasing the range picker toward previously-
    /// seen versions. `None` skips the preference walk entirely.
    pub preferred_version_selectors: Option<&'a VersionSelectors>,
    /// `minimumReleaseAge` cutoff. When present, the picker filters
    /// out any version published after this point (or fails closed
    /// with [`PickPackageFromMetaError::MissingTime`] if the
    /// packument can't be checked).
    pub published_by: Option<chrono::DateTime<chrono::Utc>>,
    /// Per-package exclude policy. A match against the package name
    /// either skips the maturity filter entirely (`AnyVersion`) or
    /// restricts it to a trusted-versions allowlist
    /// (`ExactVersions`).
    pub published_by_exclude: Option<&'a PackageVersionPolicy>,
}

/// Error from [`pick_package_from_meta`] and friends. The codes are
/// part of the public contract so the install layer's error handler
/// can switch on them by string.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PickPackageFromMetaError {
    /// `ERR_PNPM_UNPUBLISHED_PKG`: the packument has no live versions
    /// AND lists unpublished versions under `time.unpublished`.
    #[display("No versions available for {pkg_name} because it was unpublished")]
    #[diagnostic(code(ERR_PNPM_UNPUBLISHED_PKG))]
    Unpublished {
        #[error(not(source))]
        pkg_name: String,
    },
    /// `ERR_PNPM_NO_VERSIONS`: the packument has no versions at all
    /// (and no unpublished marker to disambiguate).
    #[display("No versions available for {pkg_name}. The package may be unpublished.")]
    #[diagnostic(code(ERR_PNPM_NO_VERSIONS))]
    NoVersions {
        #[error(not(source))]
        pkg_name: String,
    },
    /// `ERR_PNPM_MISSING_TIME`: `minimumReleaseAge` is active, the
    /// packument has no per-version `time`, and `modified` is
    /// missing/invalid or past the cutoff — the picker can't decide
    /// which versions are mature.
    #[display(r#"The metadata of {pkg_name} is missing the "time" field"#)]
    #[diagnostic(code(ERR_PNPM_MISSING_TIME))]
    MissingTime {
        #[error(not(source))]
        pkg_name: String,
    },
}

/// Pure picker entry point.
///
/// `pick_version_by_range` is dependency-injected so the caller can
/// pick the high-side ([`pick_version_by_version_range`]) or low-side
/// ([`pick_lowest_version_by_version_range`]) variant.
///
/// Returns:
///
/// - `Ok(Some(version))` — the picked version's (shared) manifest.
/// - `Ok(None)` — no version satisfies the spec. The orchestrator
///   layer above propagates this as "resolver returned nothing,"
///   not as an error.
/// - `Err(_)` — one of the four `PnpmError` variants above.
pub fn pick_package_from_meta<PickFn>(
    pick_version_by_range: PickFn,
    opts: &PickPackageFromMetaOptions<'_>,
    meta: &Package,
    spec: &RegistryPackageSpec,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError>
where
    PickFn: Fn(&PickVersionByVersionRangeOptions<'_>) -> Option<String>,
{
    // "Owned-after-filter" shape: when publishedBy is active and a
    // maturity filter applies, swap `meta` for the filtered view —
    // otherwise borrow the input through.
    let view: PublishedByView;
    let meta_ref: &Package = match opts.published_by {
        Some(cutoff) => {
            view = apply_published_by_policy(meta, cutoff, opts.published_by_exclude);
            mature_view(&view, meta, cutoff)?
        }
        None => meta,
    };

    if meta_ref.versions.is_empty() && opts.published_by.is_none() {
        return Err(no_versions_error(meta_ref, spec));
    }

    // An undecodable fragment behaves as if the version were absent
    // (the `PackageVersions` contract), so a pick whose winner fails
    // to hydrate retries against the remaining versions instead of
    // reporting "no match" while satisfying candidates exist. The
    // owned filtered clone only materializes on that (rare) path.
    let mut undecodable_excluded: Option<Package> = None;
    loop {
        let meta_now: &Package = undecodable_excluded.as_ref().unwrap_or(meta_ref);
        let Some(version) = pick_version(&pick_version_by_range, opts, meta_now, spec) else {
            return Ok(None);
        };
        let Some(manifest) = meta_now.versions.get(&version) else {
            if !meta_now.versions.contains_key(&version) {
                // The picked string names a version the packument
                // doesn't carry (a dangling dist-tag, an exact spec
                // for an unpublished version) — nothing to retry.
                return Ok(None);
            }
            undecodable_excluded = Some(without_version(meta_now, &version));
            continue;
        };
        return Ok(Some(pinned_manifest(manifest, meta_now)));
    }
}

/// A packument with no versions at all: either every version was
/// unpublished, or it never had any.
fn no_versions_error(meta: &Package, spec: &RegistryPackageSpec) -> PickPackageFromMetaError {
    if has_unpublished_versions(meta) {
        return PickPackageFromMetaError::Unpublished { pkg_name: spec.name.clone() };
    }
    PickPackageFromMetaError::NoVersions { pkg_name: spec.name.clone() }
}

/// GitHub registry quirk: a scoped package can be published as `@owner/foo`
/// while the per-version `name` is just `foo`. The manifest name is pinned
/// to the packument-level name.
fn pinned_manifest(manifest: Arc<PackageVersion>, meta: &Package) -> Arc<PackageVersion> {
    if meta.name.is_empty() || manifest.name == meta.name {
        return manifest;
    }
    let mut pinned = (*manifest).clone();
    pinned.name.clone_from(&meta.name);
    Arc::new(pinned)
}

/// The packument the maturity filter leaves to pick from.
///
/// A view that needs full metadata reports the missing-time error, which
/// signals the orchestrator to upgrade the fetch — unless the packument's
/// own `modified` proves no version is newer than the cutoff. That check is
/// inclusive (`<=`) to match the per-version filter in
/// [`filter_pkg_metadata_by_publish_date`]: a version published exactly at the
/// cutoff is mature.
fn mature_view<'a>(
    view: &'a PublishedByView,
    meta: &'a Package,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> Result<&'a Package, PickPackageFromMetaError> {
    if !view.needs_full_metadata {
        return Ok(view.filtered.as_deref().unwrap_or(meta));
    }
    let modified_date = meta.modified.as_deref().and_then(parse_packument_timestamp);
    match modified_date {
        Some(date) if date <= cutoff => Ok(meta),
        _ => Err(PickPackageFromMetaError::MissingTime { pkg_name: meta.name.clone() }),
    }
}

/// The version string the spec selects: itself for an exact version, the
/// tag's target for a tag, and the picker's answer for a range.
fn pick_version<PickFn>(
    pick_version_by_range: &PickFn,
    opts: &PickPackageFromMetaOptions<'_>,
    meta: &Package,
    spec: &RegistryPackageSpec,
) -> Option<String>
where
    PickFn: Fn(&PickVersionByVersionRangeOptions<'_>) -> Option<String>,
{
    match spec.spec_type {
        RegistryPackageSpecType::Version => Some(spec.fetch_spec.clone()),
        RegistryPackageSpecType::Tag => meta.dist_tag(&spec.fetch_spec).map(str::to_string),
        RegistryPackageSpecType::Range => {
            pick_version_by_range(&PickVersionByVersionRangeOptions {
                meta,
                version_range: &spec.fetch_spec,
                preferred_version_selectors: opts.preferred_version_selectors,
                published_by: opts.published_by,
            })
        }
    }
}

/// Clone `meta` minus one version — the retry step when a picked
/// version's fragment turns out to be undecodable. Slots move without
/// hydrating.
fn without_version(meta: &Package, version: &str) -> Package {
    Package {
        name: meta.name.clone(),
        // Tags pointing at the removed version go with it — the
        // latest-tag fast path would otherwise re-pick the version
        // this clone exists to exclude.
        dist_tags: meta
            .dist_tags
            .iter()
            .filter(|(_, target)| *target != version)
            .map(|(tag, target)| (tag.clone(), target.clone()))
            .collect(),
        versions: meta.versions.filtered(|candidate| candidate != version),
        time: meta.time.clone(),
        modified: meta.modified.clone(),
        etag: meta.etag.clone(),
        homepage: meta.homepage.clone(),
        mutex: Arc::default(),
        derived: DerivedPackuments::default(),
    }
}

/// Per-call inputs to the range-picker pluggable.
pub struct PickVersionByVersionRangeOptions<'a> {
    pub meta: &'a Package,
    pub version_range: &'a str,
    pub preferred_version_selectors: Option<&'a VersionSelectors>,
    /// Neither [`pick_version_by_version_range`] nor
    /// [`pick_lowest_version_by_version_range`] reads it — the
    /// filtering already happened in [`pick_package_from_meta`] —
    /// but the field stays on the options so a custom picker (e.g.
    /// the min-release-age picker) can branch on it.
    pub published_by: Option<chrono::DateTime<chrono::Utc>>,
}

/// Pick the **highest** version in `meta.versions` satisfying
/// `version_range`. Honors the `preferred_version_selectors` bias
/// when supplied, and falls back to a non-deprecated retry when the
/// top pick is deprecated and other versions are available.
pub fn pick_version_by_version_range(
    opts: &PickVersionByVersionRangeOptions<'_>,
) -> Option<String> {
    let latest = opts.meta.dist_tag("latest");

    if let Some(pick) = preferred_max_pick(opts, latest) {
        return Some(pick);
    }

    // `*` is special-cased because `semver.satisfies` rejects prereleases
    // for `*`: a package whose only version is `1.0.0-beta.1` would
    // otherwise return nothing for `*`. See pnpm/pnpm#865.
    if let Some(latest) = latest
        && (opts.version_range == "*" || semver_satisfies_loose(latest, opts.version_range))
    {
        return Some(latest.to_string());
    }

    let all_versions: Vec<&str> = opts.meta.versions.keys().map(String::as_str).collect();
    let max_pick = max_satisfying(&all_versions, opts.version_range)?;
    non_deprecated_pick(opts, &all_versions, &max_pick).or(Some(max_pick))
}

/// The highest satisfying version of the first preference group that has
/// one, with `latest` winning inside its own group.
fn preferred_max_pick(
    opts: &PickVersionByVersionRangeOptions<'_>,
    latest: Option<&str>,
) -> Option<String> {
    let selectors = opts.preferred_version_selectors.filter(|selectors| !selectors.is_empty())?;
    let groups = prioritize_preferred_versions(opts.meta, opts.version_range, Some(selectors));
    for group in groups {
        if let Some(latest) = latest
            && group.iter().any(|version| version == latest)
            && semver_satisfies_loose(latest, opts.version_range)
        {
            return Some(latest.to_string());
        }
        if let Some(pick) = max_satisfying(&group, opts.version_range) {
            return Some(pick);
        }
    }
    None
}

/// A deprecated top pick falls back to the highest non-deprecated version,
/// when the packument carries another one at all.
fn non_deprecated_pick(
    opts: &PickVersionByVersionRangeOptions<'_>,
    all_versions: &[&str],
    picked: &str,
) -> Option<String> {
    if !opts.meta.versions.is_deprecated(picked) || all_versions.len() <= 1 {
        return None;
    }
    let non_deprecated: Vec<&str> = all_versions
        .iter()
        .copied()
        .filter(|version| !opts.meta.versions.is_deprecated(version))
        .collect();
    max_satisfying(&non_deprecated, opts.version_range)
}

/// Pick the **lowest** version in `meta.versions` satisfying
/// `version_range`. Honors the `preferred_version_selectors` bias
/// when supplied.
pub fn pick_lowest_version_by_version_range(
    opts: &PickVersionByVersionRangeOptions<'_>,
) -> Option<String> {
    if let Some(selectors) = opts.preferred_version_selectors
        && !selectors.is_empty()
    {
        let groups = prioritize_preferred_versions(opts.meta, opts.version_range, Some(selectors));
        for group in groups {
            if let Some(pick) = min_satisfying(&group, opts.version_range) {
                return Some(pick);
            }
        }
    }

    let all_versions: Vec<&str> = opts.meta.versions.keys().map(String::as_str).collect();
    if opts.version_range == "*" {
        let mut parsed: Vec<(Version, &str)> = all_versions
            .iter()
            .filter_map(|raw| Version::parse(raw).ok().map(|version| (version, *raw)))
            .collect();
        parsed.sort_by(|left, right| left.0.cmp(&right.0));
        return parsed.first().map(|(_, raw)| (*raw).to_string());
    }
    min_satisfying(&all_versions, opts.version_range)
}

/// Returns the cached version only when lockfile preferences prove
/// that no version missing from the packument could tie or outrank it.
#[must_use]
pub fn pick_stable_cached_range_version(
    meta: &Package,
    version_range: &str,
    preferred_version_selectors: Option<&VersionSelectors>,
) -> Option<String> {
    let dominant = dominant_lockfile_version(version_range, preferred_version_selectors)?;
    if !meta.versions.contains_key(&dominant) {
        return None;
    }
    let picked = pick_version_by_version_range(&PickVersionByVersionRangeOptions {
        meta,
        version_range,
        preferred_version_selectors,
        published_by: None,
    })?;
    (picked == dominant).then_some(dominant)
}

fn has_unpublished_versions(meta: &Package) -> bool {
    let Some(time) = meta.time.as_ref() else { return false };
    let Some(unpublished) = time.get("unpublished") else { return false };
    unpublished
        .get("versions")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|versions| !versions.is_empty())
}

#[cfg(test)]
mod tests;
