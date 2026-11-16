use crate::{model::RegistryVersion, registry::Registry};
use miette::Result;
use semver::Version;
use std::collections::{BTreeMap, BTreeSet};

/// Versions to keep, by crate name, each list newest first without duplicates.
pub(crate) type PreferredVersions = BTreeMap<String, Vec<Version>>;

pub(crate) fn parse(
    preferred_versions: &BTreeMap<String, Vec<String>>,
) -> Result<PreferredVersions> {
    preferred_versions
        .iter()
        .map(|(name, versions)| {
            let mut versions = versions
                .iter()
                .map(|version| {
                    Version::parse(version)
                        .map_err(|error| {
                            miette::miette!("invalid preferred Cargo version {version}: {error}")
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            versions.sort_unstable_by(|left, right| right.cmp(left));
            versions.dedup();
            Ok((name.clone(), versions))
        })
        .collect()
}

/// Whether resolution may select `version` of `name`. A yanked release is
/// selectable only while the lockfile records it, as `cargo` keeps it.
pub(crate) fn is_selectable(
    preferred_versions: &PreferredVersions,
    name: &str,
    version: &RegistryVersion,
) -> bool {
    !version.yanked || contains(preferred_versions, name, &version.version)
}

pub(crate) fn contains(
    preferred_versions: &PreferredVersions,
    name: &str,
    version: &Version,
) -> bool {
    preferred_versions
        .get(name)
        .is_some_and(|versions| {
            versions
                .binary_search_by(|probe| version.cmp(probe))
                .is_ok()
        })
}

/// Drop the preferred versions `registry` does not offer, so no list is
/// longer than the crate's index entry.
pub(crate) fn retain_indexed(preferred_versions: &mut PreferredVersions, registry: &Registry) {
    preferred_versions.retain(|name, versions| {
        let indexed = registry
            .versions(name)
            .unwrap_or_default()
            .iter()
            .map(|version| &version.version)
            .collect::<BTreeSet<_>>();
        versions.retain(|version| indexed.contains(version));
        !versions.is_empty()
    });
}

#[cfg(test)]
mod tests;
