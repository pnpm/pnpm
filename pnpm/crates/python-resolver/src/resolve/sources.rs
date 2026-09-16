use super::{Package, Provider};
use miette::{IntoDiagnostic, Result};
use pep440_rs::Version;
use pep508_rs::{Requirement, VersionOrUrl};
use std::collections::BTreeSet;

pub(super) fn has_inactive_source(
    provider: &Provider<'_>,
    solution: &pubgrub::SelectedDependencies<Package, Version>,
) -> Result<bool> {
    if provider.packages.direct_urls.is_empty() {
        return Ok(false);
    }
    let mut active = BTreeSet::new();
    record_sources(provider, provider.requirements, &[], &mut active)?;
    for (package, version) in solution.iter() {
        let Package::Distribution(name, extra) = package else { continue };
        let metadata = &provider.packages.metadata[&(name.clone(), version.clone())];
        let requirements = metadata.requires_dist
            .iter()
            .map(|declared| declared.parse::<Requirement>())
            .collect::<std::result::Result<Vec<_>, _>>()
            .into_diagnostic()?;
        let extras = extra
            .clone()
            .into_iter()
            .collect::<Vec<_>>();
        record_sources(provider, &requirements, &extras, &mut active)?;
    }
    Ok(provider.packages.direct_urls
        .keys()
        .any(|name| !active.contains(name)))
}

fn record_sources(
    provider: &Provider<'_>,
    requirements: &[Requirement],
    extras: &[pep508_rs::ExtraName],
    active: &mut BTreeSet<pep508_rs::PackageName>,
) -> Result<()> {
    for requirement in requirements {
        let Some(VersionOrUrl::Url(url)) = &requirement.version_or_url else { continue };
        if !requirement.marker.evaluate(provider.environment, extras) {
            continue;
        }
        let source = crate::Source::parse(url.as_str())?;
        if provider.packages.candidates
            .get(&requirement.name)
            .is_some_and(|versions| {
                versions
                    .values()
                    .all(|candidate| candidate.matches_source(&source))
            })
        {
            active.insert(requirement.name.clone());
        }
    }
    Ok(())
}
