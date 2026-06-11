//! Pure version-picking logic over an already-fetched packument.
//!
//! Ports pnpm's
//! [`pickPackageFromMeta.ts`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts).
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

use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

use dashmap::DashMap;
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::{Range, Version};
use pacquet_config::version_policy::{PackageVersionPolicy, PolicyMatch};
use pacquet_registry::{Package, PackageVersion};
use pacquet_resolving_resolver_base::{
    VersionSelectorEntry, VersionSelectorType, VersionSelectors, parse_packument_timestamp,
};

/// Discriminator for [`RegistryPackageSpec::spec_type`]. Mirrors
/// upstream's
/// [`'tag' | 'version' | 'range'`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/parseBareSpecifier.ts#L7-L11)
/// triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryPackageSpecType {
    /// Exact version pin, e.g. `1.2.3`.
    Version,
    /// Dist-tag, e.g. `latest`, `next`.
    Tag,
    /// Semver range, e.g. `^1.0.0`.
    Range,
}

/// Parsed registry spec produced by upstream's
/// [`parseBareSpecifier`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/parseBareSpecifier.ts#L7-L12).
/// The picker (and the cache+fetch wrapper above it) consume this
/// shape; the parser that produces it is its own port and is not part
/// of this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryPackageSpec {
    pub name: String,
    pub fetch_spec: String,
    pub spec_type: RegistryPackageSpecType,
    /// Echo of the original bare specifier when the spec came from a
    /// tarball-URL parse. The resolver writes this back into
    /// `ResolveResult.normalized_bare_specifier`; the picker itself
    /// doesn't read it.
    pub normalized_bare_specifier: Option<String>,
}

/// Options bundle for [`pick_package_from_meta`]. Mirrors upstream's
/// [`PickPackageFromMetaOptions`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L21-L25).
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

/// Error from [`pick_package_from_meta`] and friends. The codes match
/// upstream's `PnpmError` shape so the install layer's error handler
/// can switch on them by string.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PickPackageFromMetaError {
    /// Mirrors upstream's
    /// [`ERR_PNPM_UNPUBLISHED_PKG`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L61):
    /// the packument has no live versions AND lists unpublished
    /// versions under `time.unpublished`.
    #[display("No versions available for {pkg_name} because it was unpublished")]
    #[diagnostic(code(ERR_PNPM_UNPUBLISHED_PKG))]
    Unpublished {
        #[error(not(source))]
        pkg_name: String,
    },
    /// Mirrors upstream's
    /// [`ERR_PNPM_NO_VERSIONS`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L63):
    /// the packument has no versions at all (and no unpublished
    /// marker to disambiguate).
    #[display("No versions available for {pkg_name}. The package may be unpublished.")]
    #[diagnostic(code(ERR_PNPM_NO_VERSIONS))]
    NoVersions {
        #[error(not(source))]
        pkg_name: String,
    },
    /// Mirrors upstream's
    /// [`ERR_PNPM_MISSING_TIME`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L112):
    /// `minimumReleaseAge` is active, the packument has no per-version
    /// `time`, and `modified` is missing/invalid or past the cutoff —
    /// the picker can't decide which versions are mature.
    #[display(r#"The metadata of {pkg_name} is missing the "time" field"#)]
    #[diagnostic(code(ERR_PNPM_MISSING_TIME))]
    MissingTime {
        #[error(not(source))]
        pkg_name: String,
    },
}

/// Pure picker entry point. Mirrors upstream's
/// [`pickPackageFromMeta`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L27-L108).
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
    // Match upstream's "owned-after-filter" shape: when publishedBy
    // is active and a maturity filter applies, swap `meta` for a
    // filtered clone — otherwise borrow the input through.
    let filtered;
    let meta_ref: &Package = match opts.published_by {
        Some(cutoff) => {
            let exclude_result = opts
                .published_by_exclude
                .map_or(PolicyMatch::No, |policy| policy.matches(&meta.name));
            if matches!(exclude_result, PolicyMatch::AnyVersion) {
                // Bare-name match — every version of this package is
                // covered by the exclude, so the maturity filter is
                // a no-op. Borrow the input through.
                meta
            } else if meta.time.is_some() {
                // Full metadata — filter by per-version `time`.
                let trusted = match &exclude_result {
                    PolicyMatch::ExactVersions(versions) => Some(versions.as_slice()),
                    _ => None,
                };
                filtered = filter_pkg_metadata_by_publish_date(meta, cutoff, trusted);
                &filtered
            } else {
                // Abbreviated metadata — no per-version `time`. Fall
                // back to the package-level `modified` shortcut: if
                // the registry says the whole package hasn't been
                // touched since the cutoff, every version is old
                // enough. Otherwise we can't decide and have to
                // signal a missing-time error to the orchestrator,
                // which then upgrades the fetch to full metadata.
                //
                // Cutoff is inclusive (`<=`) to match the per-version
                // filter in `filter_pkg_metadata_by_publish_date`: a
                // version published exactly at the cutoff is mature,
                // so `modified == cutoff` (which means no version is
                // newer than the cutoff) is also safe to shortcut.
                let modified_date = meta.modified.as_deref().and_then(parse_packument_timestamp);
                match modified_date {
                    Some(date) if date <= cutoff => meta,
                    _ => {
                        return Err(PickPackageFromMetaError::MissingTime {
                            pkg_name: meta.name.clone(),
                        });
                    }
                }
            }
        }
        None => meta,
    };

    if meta_ref.versions.is_empty() && opts.published_by.is_none() {
        // Mirrors upstream: with publishedBy off, an empty versions
        // map is either "unpublished" (when the `time.unpublished`
        // marker is present) or "no versions at all."
        if has_unpublished_versions(meta_ref) {
            return Err(PickPackageFromMetaError::Unpublished { pkg_name: spec.name.clone() });
        }
        return Err(PickPackageFromMetaError::NoVersions { pkg_name: spec.name.clone() });
    }

    // An undecodable fragment behaves as if the version were absent
    // (the `PackageVersions` contract), so a pick whose winner fails
    // to hydrate retries against the remaining versions instead of
    // reporting "no match" while satisfying candidates exist. The
    // owned filtered clone only materializes on that (rare) path.
    let mut undecodable_excluded: Option<Package> = None;
    loop {
        let meta_now: &Package = undecodable_excluded.as_ref().unwrap_or(meta_ref);
        let picked_version: Option<String> = match spec.spec_type {
            RegistryPackageSpecType::Version => Some(spec.fetch_spec.clone()),
            RegistryPackageSpecType::Tag => meta_now.dist_tag(&spec.fetch_spec).map(str::to_string),
            RegistryPackageSpecType::Range => {
                pick_version_by_range(&PickVersionByVersionRangeOptions {
                    meta: meta_now,
                    version_range: &spec.fetch_spec,
                    preferred_version_selectors: opts.preferred_version_selectors,
                    published_by: opts.published_by,
                })
            }
        };

        let Some(version) = picked_version else { return Ok(None) };
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
        if !meta_now.name.is_empty() && manifest.name != meta_now.name {
            // GitHub registry quirk: a scoped package can be published as
            // `@owner/foo` while the per-version `name` is just `foo`.
            // Match upstream's shim that pins the manifest name to the
            // packument-level name.
            let mut pinned = (*manifest).clone();
            pinned.name.clone_from(&meta_now.name);
            return Ok(Some(Arc::new(pinned)));
        }
        return Ok(Some(manifest));
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
    }
}

/// Per-call inputs to the range-picker pluggable. Mirrors upstream's
/// [`PickVersionByVersionRangeOptions`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L12-L17).
pub struct PickVersionByVersionRangeOptions<'a> {
    pub meta: &'a Package,
    pub version_range: &'a str,
    pub preferred_version_selectors: Option<&'a VersionSelectors>,
    /// Threaded through for parity with upstream. Neither
    /// [`pick_version_by_version_range`] nor
    /// [`pick_lowest_version_by_version_range`] reads it — the
    /// filtering already happened in [`pick_package_from_meta`] —
    /// but the field stays on the options so a custom picker (e.g.
    /// the one upstream's
    /// [`pickRespectingMinReleaseAge`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L111-L123)
    /// uses) can branch on it.
    pub published_by: Option<chrono::DateTime<chrono::Utc>>,
}

/// Pick the **highest** version in `meta.versions` satisfying
/// `version_range`. Honors the `preferred_version_selectors` bias
/// when supplied, and falls back to a non-deprecated retry when the
/// top pick is deprecated and other versions are available. Mirrors
/// upstream's
/// [`pickVersionByVersionRange`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L168-L203).
pub fn pick_version_by_version_range(
    opts: &PickVersionByVersionRangeOptions<'_>,
) -> Option<String> {
    let latest = opts.meta.dist_tag("latest");

    if let Some(selectors) = opts.preferred_version_selectors
        && !selectors.is_empty()
    {
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
    }

    if let Some(latest) = latest {
        // The `*` short-circuit matches upstream — `semver.satisfies`
        // rejects prereleases for `*`, so a package whose only
        // version is `1.0.0-beta.1` would have `*` return nothing
        // without this branch. See pnpm/pnpm#865.
        if opts.version_range == "*" || semver_satisfies_loose(latest, opts.version_range) {
            return Some(latest.to_string());
        }
    }

    let all_versions: Vec<&str> = opts.meta.versions.keys().map(String::as_str).collect();
    let max_pick = max_satisfying(&all_versions, opts.version_range);

    // Deprecated-fallback: if the picked max is deprecated AND the
    // packument has another version, try again with only the
    // non-deprecated subset. Matches upstream's loop at
    // pickPackageFromMeta.ts#L194-L201.
    if let Some(ref picked) = max_pick {
        let picked_meta = opts.meta.versions.get(picked);
        let picked_is_deprecated = picked_meta.is_some_and(|version| version.deprecated.is_some());
        if picked_is_deprecated && all_versions.len() > 1 {
            let non_deprecated: Vec<&str> = opts
                .meta
                .versions
                .iter()
                .filter(|(_, manifest)| manifest.deprecated.is_none())
                .map(|(name, _)| name.as_str())
                .collect();
            if let Some(non_deprecated_max) = max_satisfying(&non_deprecated, opts.version_range) {
                return Some(non_deprecated_max);
            }
        }
    }

    max_pick
}

/// Pick the **lowest** version in `meta.versions` satisfying
/// `version_range`. Honors the `preferred_version_selectors` bias
/// when supplied. Mirrors upstream's
/// [`pickLowestVersionByVersionRange`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L150-L166).
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

/// Filter a packument to versions published at or before `cutoff`,
/// then rewrite each `dist-tag` to the highest within-cutoff version
/// that still belongs to the tag's original "family" (same major
/// for non-`latest` tags, same prerelease/release status, and
/// preferring non-deprecated versions when both are present).
/// Mirrors upstream's
/// [`filterPkgMetadataByPublishDate`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/registry/pkg-metadata-filter/src/index.ts#L5-L82).
///
/// Panics if `meta.time` is `None` — the caller (the publishedBy
/// branch in [`pick_package_from_meta`]) only invokes this with full
/// metadata. The abbreviated-metadata path takes the `meta.modified`
/// shortcut above and never reaches this function.
#[must_use]
pub fn filter_pkg_metadata_by_publish_date(
    meta: &Package,
    cutoff: chrono::DateTime<chrono::Utc>,
    trusted_versions: Option<&[String]>,
) -> Package {
    let time = meta.time.as_ref().expect(
        "filter_pkg_metadata_by_publish_date called without `time`; \
         caller must check before invoking",
    );

    // Decide on version strings + the `time` map alone; slots move as
    // raw fragments, so the filter never hydrates a manifest.
    let versions_within_date = meta.versions.filtered(|version| {
        let mature = time
            .get(version)
            .and_then(serde_json::Value::as_str)
            .and_then(parse_packument_timestamp)
            .is_some_and(|date| date <= cutoff);
        let trusted =
            trusted_versions.is_some_and(|allow| allow.iter().any(|allowed| allowed == version));
        mature || trusted
    });

    let mut dist_tags_within_date = std::collections::HashMap::new();
    for (tag, version) in &meta.dist_tags {
        if versions_within_date.contains_key(version) {
            dist_tags_within_date.insert(tag.clone(), version.clone());
            continue;
        }
        let Ok(original) = Version::parse(version) else { continue };
        let original_is_prerelease = !original.pre_release.is_empty();
        let mut best_version: Option<(Version, &String)> = None;
        for candidate_raw in versions_within_date.keys() {
            let Ok(candidate) = Version::parse(candidate_raw) else { continue };
            if tag != "latest" && candidate.major != original.major {
                continue;
            }
            if candidate.pre_release.is_empty() == original_is_prerelease {
                continue;
            }
            match best_version {
                None => best_version = Some((candidate, candidate_raw)),
                Some((ref best, best_raw)) => {
                    let best_deprecated = versions_within_date
                        .get(best_raw)
                        .is_some_and(|manifest| manifest.deprecated.is_some());
                    let candidate_deprecated = versions_within_date
                        .get(candidate_raw)
                        .is_some_and(|manifest| manifest.deprecated.is_some());
                    let candidate_wins = (candidate > *best
                        && best_deprecated == candidate_deprecated)
                        || (best_deprecated && !candidate_deprecated);
                    if candidate_wins {
                        best_version = Some((candidate, candidate_raw));
                    }
                }
            }
        }
        if let Some((_, best_raw)) = best_version {
            dist_tags_within_date.insert(tag.clone(), best_raw.clone());
        }
    }

    Package {
        name: meta.name.clone(),
        dist_tags: dist_tags_within_date,
        versions: versions_within_date,
        time: meta.time.clone(),
        modified: meta.modified.clone(),
        etag: meta.etag.clone(),
        homepage: meta.homepage.clone(),
        mutex: std::sync::Arc::clone(&meta.mutex),
    }
}

/// Group versions by weight (highest weight first); each group is
/// the input to a single max/min-satisfying call. Mirrors
/// upstream's
/// [`prioritizePreferredVersions`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L205-L249).
fn prioritize_preferred_versions(
    meta: &Package,
    version_range: &str,
    preferred_version_selectors: Option<&VersionSelectors>,
) -> Vec<Vec<String>> {
    let mut prioritizer = PreferredVersionsPrioritizer::default();

    // Seed every range-satisfying version at weight 0. JS treats 0
    // as falsy, so a later positive-weight `add` overwrites this
    // sentinel rather than summing with it — preserved below in
    // [`PreferredVersionsPrioritizer::add`].
    for version in meta.versions.keys() {
        if semver_satisfies_loose(version, version_range) {
            prioritizer.add(version.clone(), 0);
        }
    }

    if let Some(selectors) = preferred_version_selectors {
        for (preferred_selector, entry) in selectors {
            if preferred_selector == version_range {
                continue;
            }
            let (selector_type, weight) = match entry {
                VersionSelectorEntry::Plain(selector_type) => (*selector_type, 1),
                VersionSelectorEntry::Weighted(weighted) => {
                    (weighted.selector_type, weighted.weight)
                }
            };
            match selector_type {
                VersionSelectorType::Tag => {
                    if let Some(version) = meta.dist_tag(preferred_selector) {
                        prioritizer.add(version.to_string(), weight);
                    }
                }
                VersionSelectorType::Range => {
                    for version in meta.versions.keys() {
                        if semver_satisfies_loose(version, preferred_selector) {
                            prioritizer.add(version.clone(), weight);
                        }
                    }
                }
                VersionSelectorType::Version => {
                    if meta.versions.contains_key(preferred_selector) {
                        prioritizer.add(preferred_selector.clone(), weight);
                    }
                }
            }
        }
    }

    prioritizer.versions_by_priority()
}

/// Group-by-weight accumulator. Matches upstream's JS class
/// [`PreferredVersionsPrioritizer`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L251-L273)
/// — including the quirk that weight `0` acts as a sentinel a later
/// non-zero `add` overwrites rather than sums with.
#[derive(Default)]
struct PreferredVersionsPrioritizer {
    preferred_versions: BTreeMap<String, u32>,
}

impl PreferredVersionsPrioritizer {
    fn add(&mut self, version: String, weight: u32) {
        let entry = self.preferred_versions.entry(version).or_insert(0);
        if *entry == 0 {
            // JS truthiness: `0` is falsy, so a later positive
            // weight replaces the seed. Once non-zero, further
            // adds sum normally.
            *entry = weight;
        } else {
            *entry += weight;
        }
    }

    fn versions_by_priority(&self) -> Vec<Vec<String>> {
        let mut by_weight: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        for (version, weight) in &self.preferred_versions {
            by_weight.entry(*weight).or_default().push(version.clone());
        }
        // Highest weight first. BTreeMap iterates lowest→highest, so
        // reverse explicitly.
        by_weight.into_iter().rev().map(|(_, group)| group).collect()
    }
}

/// Process-global cache of parsed [`Range`]s keyed by their source
/// string. Mirrors upstream's
/// [`semverRangeCache`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackageFromMeta.ts#L123-L148):
/// most installs hit the same handful of ranges thousands of times
/// (the `*` from a CLI add, the `^X` from manifest entries, the few
/// dist-tag fall-backs in `preferred_version_selectors`), and reparsing
/// each is the picker's hottest cost. The cache stores `Option<Arc<Range>>`
/// so the parse error case ("range is unparsable") is memoized too —
/// pickers fall through to the next candidate without retrying the
/// parse.
///
/// `DashMap` (not `Mutex<HashMap>`) keeps lookups lock-free under the
/// fan-out the deps-resolver runs concurrently.
static RANGE_CACHE: LazyLock<DashMap<String, Option<Arc<Range>>>> = LazyLock::new(DashMap::new);

fn cached_range(range: &str) -> Option<Arc<Range>> {
    if let Some(entry) = RANGE_CACHE.get(range) {
        // `entry` is a `dashmap::Ref` guard around the stored
        // `Option<Arc<Range>>`. `value()` projects out the `&Option<...>`
        // so the clone runs on the inner value (Arc bump + Option clone),
        // not on the guard.
        return entry.value().clone();
    }
    let parsed = Range::parse(range).ok().map(Arc::new);
    RANGE_CACHE.insert(range.to_string(), parsed.clone());
    parsed
}

/// Check whether `version` satisfies `range` under node-semver's
/// loose grammar, reusing a cached [`Range`] parse when possible.
/// A parse failure on either input is treated as "doesn't satisfy"
/// so the picker can fall through to the next candidate instead of
/// crashing.
fn semver_satisfies_loose(version: &str, range: &str) -> bool {
    let Ok(parsed_version) = Version::parse(version) else { return false };
    let Some(parsed_range) = cached_range(range) else { return false };
    parsed_version.satisfies(&parsed_range)
}

fn max_satisfying<Raw: AsRef<str>>(versions: &[Raw], range: &str) -> Option<String> {
    let parsed_range = cached_range(range)?;
    let mut best: Option<(Version, String)> = None;
    for version in versions {
        let Ok(parsed) = Version::parse(version.as_ref()) else { continue };
        if !parsed.satisfies(&parsed_range) {
            continue;
        }
        match &best {
            Some((current, _)) if current >= &parsed => {}
            _ => best = Some((parsed, version.as_ref().to_string())),
        }
    }
    best.map(|(_, raw)| raw)
}

fn min_satisfying<Raw: AsRef<str>>(versions: &[Raw], range: &str) -> Option<String> {
    let parsed_range = cached_range(range)?;
    let mut best: Option<(Version, String)> = None;
    for version in versions {
        let Ok(parsed) = Version::parse(version.as_ref()) else { continue };
        if !parsed.satisfies(&parsed_range) {
            continue;
        }
        match &best {
            Some((current, _)) if current <= &parsed => {}
            _ => best = Some((parsed, version.as_ref().to_string())),
        }
    }
    best.map(|(_, raw)| raw)
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
