use miette::Result;
use semver::Version;
use std::collections::BTreeMap;

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
            Ok((name.clone(), versions))
        })
        .collect()
}

pub(crate) fn contains(
    preferred_versions: &PreferredVersions,
    name: &str,
    version: &Version,
) -> bool {
    preferred_versions
        .get(name)
        .is_some_and(|versions| versions.contains(version))
}
