//! Port of pnpm's
//! [`@pnpm/lockfile.preferred-versions`](https://github.com/pnpm/pnpm/blob/097983fbca/lockfile/preferred-versions/src/index.ts).
//!
//! Seeds the [`PreferredVersions`] map the deps-resolver consults to
//! break version-pick ties: every spec from a project manifest gets a
//! [`DIRECT_DEP_SELECTOR_WEIGHT`] entry, every concrete `name@version`
//! pinned by the wanted lockfile gets a [`EXISTING_VERSION_SELECTOR_WEIGHT`]
//! entry, and an entry that appears in both buckets has its weight bumped
//! by the lockfile weight so it outranks single-source matches.

use std::collections::HashMap;

use pacquet_lockfile::{PackageKey, SnapshotEntry};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{
    DIRECT_DEP_SELECTOR_WEIGHT, EXISTING_VERSION_SELECTOR_WEIGHT, PreferredVersions,
    VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight,
};

mod version_selector_type;

pub use version_selector_type::get_version_selector_type;

/// Build a [`PreferredVersions`] map from the wanted lockfile's
/// `snapshots:` block plus every importer manifest.
///
/// Mirrors upstream's
/// [`getPreferredVersionsFromLockfileAndManifests`](https://github.com/pnpm/pnpm/blob/097983fbca/lockfile/preferred-versions/src/index.ts#L13-L33):
///
/// 1. For each manifest, walk dev + prod + optional dependencies and
///    record the spec with [`DIRECT_DEP_SELECTOR_WEIGHT`].
/// 2. For each snapshot, fold in the `(name, version)` pair with
///    [`EXISTING_VERSION_SELECTOR_WEIGHT`]; a pre-existing direct-dep
///    selector for the same exact version has its weight bumped by
///    `EXISTING_VERSION_SELECTOR_WEIGHT` instead of being overwritten.
///
/// Pass `snapshots = None` when the wanted lockfile is absent (e.g.
/// the `install-without-lockfile` path); only manifest-derived entries
/// are produced.
#[must_use]
pub fn get_preferred_versions_from_lockfile_and_manifests(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    manifests: &[&PackageManifest],
) -> PreferredVersions {
    let mut preferred: PreferredVersions = PreferredVersions::new();
    for manifest in manifests {
        for (name, spec) in manifest.dependencies([
            DependencyGroup::Dev,
            DependencyGroup::Prod,
            DependencyGroup::Optional,
        ]) {
            let Some(selector_type) = get_version_selector_type(spec) else { continue };
            preferred.entry(name.to_string()).or_default().insert(
                spec.to_string(),
                VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
                    selector_type,
                    weight: DIRECT_DEP_SELECTOR_WEIGHT,
                }),
            );
        }
    }
    if let Some(snapshots) = snapshots {
        add_preferred_versions_from_lockfile(snapshots, &mut preferred);
    }
    preferred
}

/// Fold every `(name, version)` pair from the lockfile snapshots into
/// `preferred`, bumping the weight of pre-existing direct-dep entries
/// rather than overwriting them. The snapshot map can contain multiple
/// entries with identical `(name, version)` (different peer suffixes);
/// deduping by the `(name, version)` pair first prevents a popular
/// package from getting its weight multiplied per peer-suffix copy —
/// mirrors upstream's `uniqueNameVersions` set construction at
/// [`index.ts:41-46`](https://github.com/pnpm/pnpm/blob/097983fbca/lockfile/preferred-versions/src/index.ts#L41-L46).
fn add_preferred_versions_from_lockfile(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    preferred: &mut PreferredVersions,
) {
    let mut unique_name_versions: HashMap<String, std::collections::HashSet<String>> =
        HashMap::new();
    for key in snapshots.keys() {
        let name = key.name.to_string();
        // The lockfile records `file:`-protocol deps with a non-semver
        // version part; upstream's `nameVerFromPkgSnapshot` returns
        // those as the raw `file:` string, but the preferred-versions
        // map only feeds the semver picker — adding a `file:` entry
        // would either confuse the picker or be silently ignored
        // depending on the call site. Skip them defensively: the
        // versioned snapshots are the only useful seeds for the
        // version picker.
        let Some(version) = key.suffix.version_semver() else { continue };
        unique_name_versions.entry(name).or_default().insert(version.to_string());
    }

    for (name, versions) in unique_name_versions {
        let bucket = preferred.entry(name.clone()).or_default();
        for version in versions {
            match bucket.get(&version) {
                None => {
                    bucket.insert(
                        version,
                        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
                            selector_type: VersionSelectorType::Version,
                            weight: EXISTING_VERSION_SELECTOR_WEIGHT,
                        }),
                    );
                }
                Some(existing) => {
                    let existing_selector_type = match existing {
                        VersionSelectorEntry::Plain(ty) => *ty,
                        VersionSelectorEntry::Weighted(w) => w.selector_type,
                    };
                    // The lookup was for an exact version — the
                    // existing entry came from a direct-dep selector
                    // typed as `Version` (anything else means our state
                    // is corrupted, mirroring upstream's throw).
                    assert!(
                        matches!(existing_selector_type, VersionSelectorType::Version),
                        "Encountered unexpected version selector '{existing_selector_type:?}' for dependency '{name}@{version}'",
                    );
                    let bumped =
                        add_weight_to_version_selector(existing, EXISTING_VERSION_SELECTOR_WEIGHT);
                    bucket.insert(version, VersionSelectorEntry::Weighted(bumped));
                }
            }
        }
    }
}

/// Bump a selector's weight by `weight`, lifting a `Plain` selector
/// to `Weighted(weight + 1)` (matches upstream's `weight + 1` for the
/// string case) and adding `weight` to an existing weighted entry.
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
