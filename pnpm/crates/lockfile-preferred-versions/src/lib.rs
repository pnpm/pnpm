//! Seeds the [`PreferredVersions`] map the deps-resolver consults to
//! break version-pick ties: every spec from a project manifest gets a
//! [`DIRECT_DEP_SELECTOR_WEIGHT`] entry (a `catalog:` spec contributes the
//! catalog entry it names), every concrete `name@version`
//! pinned by the wanted lockfile gets a [`EXISTING_VERSION_SELECTOR_WEIGHT`]
//! entry, and an entry that appears in both buckets has its weight bumped
//! by the lockfile weight so it outranks single-source matches.

pub use version_selector_type::get_version_selector_type;

use std::{borrow::Cow, collections::HashMap};

use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionResult, WantedDependency, resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{
    DIRECT_DEP_SELECTOR_WEIGHT, EXISTING_VERSION_SELECTOR_WEIGHT, PreferredVersions,
    VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight,
};
use rayon::prelude::*;

mod version_selector_type;

/// The importer manifests whose direct-dependency specs seed the
/// preferences, and the catalogs to resolve them against.
#[derive(Clone, Copy)]
pub struct DirectSpecs<'a> {
    pub manifests: &'a [&'a PackageManifest],
    pub catalogs: &'a Catalogs,
}

impl<'a> DirectSpecs<'a> {
    /// Specs from `manifests` with no catalogs, so a `catalog:` spec among
    /// them seeds nothing.
    #[must_use]
    pub fn without_catalogs(manifests: &'a [&'a PackageManifest]) -> Self {
        static NO_CATALOGS: Catalogs = Catalogs::new();
        DirectSpecs { manifests, catalogs: &NO_CATALOGS }
    }

    /// `None` for a `catalog:` spec without a usable entry.
    fn effective_spec<'spec>(&self, name: &str, spec: &'spec str) -> Option<Cow<'spec, str>> {
        if parse_catalog_protocol(spec).is_none() {
            return Some(Cow::Borrowed(spec));
        }
        let wanted = WantedDependency { alias: name.to_string(), bare_specifier: spec.to_string() };
        match resolve_from_catalog(self.catalogs, &wanted, CatalogAnchor::AsWritten) {
            CatalogResolutionResult::Found(found) => Some(Cow::Owned(found.resolution.specifier)),
            CatalogResolutionResult::Misconfiguration(_) | CatalogResolutionResult::Unused => None,
        }
    }
}

/// Build a [`PreferredVersions`] map from the wanted lockfile's
/// `snapshots:` block plus every importer manifest.
///
/// Pass `snapshots = None` when the wanted lockfile is absent (e.g.
/// the `install-without-lockfile` path); only manifest-derived entries
/// are produced.
#[must_use]
pub fn get_preferred_versions_from_lockfile_and_manifests(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    direct: DirectSpecs<'_>,
) -> PreferredVersions {
    get_preferred_versions_from_lockfile_and_manifests_excluding(snapshots, direct, &|_| false)
}

/// Build a [`PreferredVersions`] map while withholding every lockfile pin
/// `withheld` accepts. Manifest-derived preferences remain workspace-wide.
#[must_use]
pub fn get_preferred_versions_from_lockfile_and_manifests_excluding(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    direct: DirectSpecs<'_>,
    withheld: &dyn Fn(&PackageKey) -> bool,
) -> PreferredVersions {
    // Each manifest's selector classification (semver parses, mostly)
    // is independent, so a workspace-scale manifest list fans out
    // across the rayon pool. Every entry is a pure function of its
    // `(name, spec)` pair — same selector type, same weight — so the
    // reduce's merge order is immaterial: colliding inserts write the
    // same value the serial loop would.
    let mut preferred: PreferredVersions = direct.manifests
        .par_iter()
        .map(|manifest| {
            let mut preferred = PreferredVersions::new();
            for (name, spec) in manifest.dependencies([
                DependencyGroup::Dev,
                DependencyGroup::Prod,
                DependencyGroup::Optional,
            ]) {
                let Some(spec) = direct.effective_spec(name, spec) else { continue };
                let Some(selector_type) = get_version_selector_type(&spec) else { continue };
                preferred
                    .entry(name.to_string())
                    .or_default()
                    .insert(
                        spec.into_owned(),
                        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
                            selector_type,
                            weight: DIRECT_DEP_SELECTOR_WEIGHT,
                        }),
                    );
            }
            preferred
        })
        .reduce(PreferredVersions::new, |mut merged, next| {
            for (name, selectors) in next {
                merged
                    .entry(name)
                    .or_default()
                    .extend(selectors);
            }
            merged
        });
    if let Some(snapshots) = snapshots {
        add_preferred_versions_from_lockfile(snapshots, withheld, &mut preferred);
    }
    preferred
}

/// Fold every `(name, version)` pair from the lockfile snapshots into
/// `preferred`, bumping the weight of pre-existing direct-dep entries
/// rather than overwriting them.
fn add_preferred_versions_from_lockfile(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    withheld: &dyn Fn(&PackageKey) -> bool,
    preferred: &mut PreferredVersions,
) {
    let mut unique_name_versions: HashMap<String, std::collections::HashSet<String>> =
        HashMap::new();
    for key in snapshots.keys() {
        if withheld(key) {
            continue;
        }
        let name = key.name.to_string();
        // The lockfile records `file:`-protocol deps with a non-semver
        // version part. The preferred-versions map only feeds the semver
        // picker — adding a `file:` entry would either confuse the picker
        // or be silently ignored depending on the call site. Skip them
        // defensively: the versioned snapshots are the only useful seeds
        // for the version picker.
        let Some(version) = key.suffix.version_semver() else { continue };
        unique_name_versions
            .entry(name)
            .or_default()
            .insert(version.to_string());
    }

    for (name, versions) in unique_name_versions {
        let bucket = preferred.entry(name.clone()).or_default();
        for version in versions {
            let entry = weighted_lockfile_version(bucket.get(&version), &name, &version);
            bucket.insert(version, entry);
        }
    }
}

/// The entry one lockfile-seeded version gets: a fresh weighted selector, or
/// the existing one with this seed's weight added.
///
/// The lookup was for an exact version, so an existing entry came from a
/// direct-dep selector typed as `Version`; anything else means the state is
/// corrupted, which the assertion catches.
fn weighted_lockfile_version(
    existing: Option<&VersionSelectorEntry>,
    name: &str,
    version: &str,
) -> VersionSelectorEntry {
    let Some(existing) = existing else {
        return VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: EXISTING_VERSION_SELECTOR_WEIGHT,
        });
    };
    let existing_selector_type = match existing {
        VersionSelectorEntry::Plain(selector_type) => *selector_type,
        VersionSelectorEntry::Weighted(weighted) => weighted.selector_type,
    };
    assert!(
        matches!(existing_selector_type, VersionSelectorType::Version),
        "Encountered unexpected version selector '{existing_selector_type:?}' for dependency '{name}@{version}'",
    );
    VersionSelectorEntry::Weighted(add_weight_to_version_selector(
        existing,
        EXISTING_VERSION_SELECTOR_WEIGHT,
    ))
}

/// Bump a selector's weight by `weight`, lifting a `Plain` selector
/// to `Weighted(weight + 1)` (the `weight + 1` for the bare-string
/// case) and adding `weight` to an existing weighted entry.
fn add_weight_to_version_selector(
    selector: &VersionSelectorEntry,
    weight: u32,
) -> VersionSelectorWithWeight {
    match selector {
        VersionSelectorEntry::Plain(selector_type) => {
            VersionSelectorWithWeight { selector_type: *selector_type, weight: weight + 1 }
        }
        VersionSelectorEntry::Weighted(existing) => VersionSelectorWithWeight {
            selector_type: existing.selector_type,
            weight: existing.weight + weight,
        },
    }
}

#[cfg(test)]
mod tests;
