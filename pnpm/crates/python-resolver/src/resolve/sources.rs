use super::{Package, Provider};
use crate::{Packages, WheelMetadata};
use miette::{IntoDiagnostic, Result, bail};
use pep440_rs::Version;
use pep508_rs::{ExtraName, MarkerEnvironment, PackageName, Requirement, VersionOrUrl};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct SourceGraph {
    active: BTreeSet<PackageName>,
    visited: BTreeSet<Package>,
    frontier: Vec<Package>,
    pending: BTreeMap<PackageName, BTreeSet<Package>>,
}

pub(super) fn has_inactive_source(
    provider: &Provider<'_>,
    solution: &pubgrub::SelectedDependencies<Package, Version>,
) -> Result<bool> {
    if provider.packages.direct_urls.is_empty() {
        return Ok(false);
    }
    let mut graph = SourceGraph { frontier: vec![Package::Root], ..SourceGraph::default() };
    while let Some(package) = graph.frontier.pop() {
        if !graph.can_visit(provider, &package) {
            continue;
        }
        let (requirements, extras) = selected_requirements(provider, solution, &package)?;
        graph.enqueue_requirements(&requirements, &extras, provider);
    }
    Ok(provider.packages.direct_urls
        .keys()
        .any(|name| !graph.active.contains(name)))
}

impl SourceGraph {
    fn can_visit(&mut self, provider: &Provider<'_>, package: &Package) -> bool {
        if let Package::Distribution(name, _) = package
            && provider.packages.direct_urls.contains_key(name)
            && !self.active.contains(name)
        {
            self.pending
                .entry(name.clone())
                .or_default()
                .insert(package.clone());
            return false;
        }
        self.visited.insert(package.clone())
    }

    fn enqueue_requirements(
        &mut self,
        requirements: &[Requirement],
        extras: &[ExtraName],
        provider: &Provider<'_>,
    ) {
        for requirement in requirements {
            if !requirement.marker.evaluate(provider.environment, extras) {
                continue;
            }
            if matches!(requirement.version_or_url, Some(VersionOrUrl::Url(_))) {
                self.active.insert(requirement.name.clone());
                self.frontier.extend(self.pending.remove(&requirement.name).unwrap_or_default());
            }
            self.frontier.extend(
                std::iter::once(None)
                    .chain(
                        provider
                            .requirement_extras(requirement, extras)
                            .into_iter()
                            .map(Some),
                    )
                    .map(|extra| Package::Distribution(requirement.name.clone(), extra)),
            );
        }
    }
}

fn selected_requirements(
    provider: &Provider<'_>,
    solution: &pubgrub::SelectedDependencies<Package, Version>,
    package: &Package,
) -> Result<(Vec<Requirement>, Vec<ExtraName>)> {
    let Package::Distribution(name, extra) = package else {
        return Ok((provider.requirements.to_vec(), Vec::new()));
    };
    let version = solution.get(package).expect("a satisfied requirement was selected");
    let metadata = &provider.packages.metadata[&(name.clone(), version.clone())];
    parsed_requirements(metadata, extra.as_ref())
}

/// Sources reached from pinned candidates whose metadata is already known.
/// Missing metadata stops traversal until that active source has been built.
pub fn active_locked_sources(
    packages: &Packages,
    requirements: &[Requirement],
    environment: &MarkerEnvironment,
) -> Result<BTreeSet<PackageName>> {
    let provider = Provider { packages, requirements, environment };
    let mut graph = SourceGraph { frontier: vec![Package::Root], ..SourceGraph::default() };
    while let Some(package) = graph.frontier.pop() {
        if !graph.can_visit(&provider, &package) {
            continue;
        }
        let Some((requirements, extras)) = known_locked_requirements(&provider, &package)? else {
            continue;
        };
        verify_locked_sources(&provider, &requirements, &extras)?;
        graph.enqueue_requirements(&requirements, &extras, &provider);
    }
    Ok(graph.active)
}

fn known_locked_requirements(
    provider: &Provider<'_>,
    package: &Package,
) -> Result<Option<(Vec<Requirement>, Vec<ExtraName>)>> {
    let Package::Distribution(name, extra) = package else {
        return Ok(Some((provider.requirements.to_vec(), Vec::new())));
    };
    let Some(versions) = provider.packages.candidates.get(name) else { return Ok(None) };
    if versions.len() != 1 {
        bail!("Python lockfile must pin a single version of {name}");
    }
    let version = versions
        .keys()
        .next()
        .expect("one pinned version");
    let Some(metadata) = provider.packages.metadata.get(&(name.clone(), version.clone())) else {
        return Ok(None);
    };
    parsed_requirements(metadata, extra.as_ref()).map(Some)
}

fn verify_locked_sources(
    provider: &Provider<'_>,
    requirements: &[Requirement],
    extras: &[ExtraName],
) -> Result<()> {
    for requirement in requirements {
        if !requirement.marker.evaluate(provider.environment, extras) {
            continue;
        }
        let Some(VersionOrUrl::Url(url)) = &requirement.version_or_url else { continue };
        let source = crate::Source::parse(url.as_str())?;
        if !provider.packages.candidates
            .get(&requirement.name)
            .is_some_and(|versions| {
                !versions.is_empty()
                    && versions
                        .values()
                        .all(|candidate| candidate.matches_source(&source))
            })
        {
            bail!("Python lockfile does not satisfy the source of {}", requirement.name);
        }
    }
    Ok(())
}

fn parsed_requirements(
    metadata: &WheelMetadata,
    extra: Option<&ExtraName>,
) -> Result<(Vec<Requirement>, Vec<ExtraName>)> {
    let requirements = metadata.requires_dist
        .iter()
        .map(|declared| declared.parse::<Requirement>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .into_diagnostic()?;
    Ok((requirements, extra.cloned().into_iter().collect()))
}
