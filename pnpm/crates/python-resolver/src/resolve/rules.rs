use super::{Needed, Provider};
use pep440_rs::Version;
use pep508_rs::{ExtraName, Requirement, VersionOrUrl};
use pubgrub::Ranges;

impl Provider<'_> {
    pub(super) fn requirement_range(
        &self,
        requirement: &Requirement,
        extras: &[ExtraName],
    ) -> std::result::Result<Ranges<Version>, Needed> {
        let overrides = self.packages.overrides
            .iter()
            .filter(|replacement| {
                replacement.name == requirement.name
                    && replacement.marker.evaluate(self.environment, extras)
            });
        let overridden = overrides.clone().next().is_some();
        let specifiers = overrides
            .chain(std::iter::once(requirement).filter(|_| !overridden))
            .map(requirement_specifiers)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let additional = self.packages.constraints
            .iter()
            .filter(|constraint| {
                constraint.name == requirement.name
                    && constraint.marker.evaluate(self.environment, extras)
            })
            .map(requirement_specifiers)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let candidates = self.packages.candidates
            .get(&requirement.name)
            .ok_or_else(|| Needed::Candidates(requirement.name.clone()))?;
        let matched = candidates
            .keys()
            .filter(|version| matches_specifiers(version, &specifiers, &additional))
            .collect::<Vec<_>>();
        let allow_prerelease = permits_prereleases(&specifiers, &additional)
            || matched.iter().all(|version| version.any_prerelease());
        Ok(matched
            .into_iter()
            .filter(|version| allow_prerelease || !version.any_prerelease())
            .fold(Ranges::empty(), |range, version| {
                range.union(&Ranges::singleton(version.clone()))
            }))
    }

    pub(super) fn requirement_extras(
        &self,
        requirement: &Requirement,
        extras: &[ExtraName],
    ) -> Vec<ExtraName> {
        let overrides = self.packages.overrides
            .iter()
            .filter(|replacement| {
                replacement.name == requirement.name
                    && replacement.marker.evaluate(self.environment, extras)
            });
        if overrides.clone().next().is_none() {
            return requirement.extras.clone();
        }
        overrides
            .flat_map(|replacement| &replacement.extras)
            .cloned()
            .collect()
    }
}

fn matches_specifiers(
    version: &Version,
    specifiers: &[Option<&pep440_rs::VersionSpecifiers>],
    additional: &[Option<&pep440_rs::VersionSpecifiers>],
) -> bool {
    specifiers
        .iter()
        .chain(additional)
        .all(|specifiers| specifiers.is_none_or(|specifiers| specifiers.contains(version)))
}

fn permits_prereleases(
    specifiers: &[Option<&pep440_rs::VersionSpecifiers>],
    additional: &[Option<&pep440_rs::VersionSpecifiers>],
) -> bool {
    specifiers
        .iter()
        .chain(additional)
        .any(|specifiers| {
            specifiers.is_some_and(|specifiers| {
                specifiers.iter().any(pep440_rs::VersionSpecifier::any_prerelease)
            })
        })
}

fn requirement_specifiers(
    requirement: &Requirement,
) -> std::result::Result<Option<&pep440_rs::VersionSpecifiers>, Needed> {
    Ok(match &requirement.version_or_url {
        Some(VersionOrUrl::VersionSpecifier(specifiers)) => Some(specifiers),
        None => None,
        Some(VersionOrUrl::Url(_)) => None,
    })
}
