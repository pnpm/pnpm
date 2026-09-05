use super::{manifest::parse_requirement, registry::Registry};
use miette::{Result, bail};
use pep440_rs::Version;
use pep508_rs::{ExtraName, MarkerEnvironment, PackageName, Requirement, VersionOrUrl};
use pnpm_reporter::Reporter as InstallReporter;
use pubgrub::{
    DefaultStringReporter, Dependencies, DependencyConstraints, DependencyProvider, DerivationTree,
    PackageResolutionStatistics, PubGrubError, Ranges, Reporter as _,
};
use std::{collections::BTreeMap, fmt};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Package {
    Root,
    Distribution(PackageName, Option<ExtraName>),
}

impl fmt::Display for Package {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => formatter.write_str("Python project"),
            Self::Distribution(name, None) => name.fmt(formatter),
            Self::Distribution(name, Some(extra)) => write!(formatter, "{name}[{extra}]"),
        }
    }
}

#[derive(Debug)]
enum Needed {
    Index(PackageName),
    Wheel(PackageName, Version),
    Invalid(String),
}

impl fmt::Display for Needed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Needed {}

struct Provider<'a> {
    registry: &'a Registry<'a>,
    requirements: &'a [Requirement],
    environment: &'a MarkerEnvironment,
}

impl DependencyProvider for Provider<'_> {
    type P = Package;
    type V = Version;
    type VS = Ranges<Version>;
    type M = String;
    type Err = Needed;
    type Priority = u32;

    fn prioritize(
        &self,
        _: &Package,
        _: &Self::VS,
        statistics: &PackageResolutionStatistics,
    ) -> u32 {
        statistics.conflict_count()
    }

    fn choose_version(
        &self,
        package: &Package,
        range: &Self::VS,
    ) -> std::result::Result<Option<Version>, Needed> {
        let Package::Distribution(name, _) = package else { return Ok(Some(Version::new([0]))) };
        let versions =
            self.registry.candidates.get(name).ok_or_else(|| Needed::Index(name.clone()))?;
        Ok(versions.keys().rev().find(|version| range.contains(version)).cloned())
    }

    fn get_dependencies(
        &self,
        package: &Package,
        version: &Version,
    ) -> std::result::Result<Dependencies<Package, Self::VS, String>, Needed> {
        let mut constraints = BTreeMap::<Package, Ranges<Version>>::new();
        match package {
            Package::Root => self.constraints(self.requirements, &[], &mut constraints)?,
            Package::Distribution(name, extra) => {
                let wheel = self
                    .registry
                    .wheels
                    .get(&(name.clone(), version.clone()))
                    .ok_or_else(|| Needed::Wheel(name.clone(), version.clone()))?;
                if let Some(specifier) = &wheel.metadata.requires_python {
                    let specifier: pep440_rs::VersionSpecifiers =
                        specifier.parse().map_err(|error| {
                            Needed::Invalid(format!("invalid Requires-Python: {error}"))
                        })?;
                    if !specifier.contains(self.environment.python_full_version()) {
                        return Ok(Dependencies::Unavailable(
                            "incompatible Python interpreter".to_string(),
                        ));
                    }
                }
                let extras = extra.clone().into_iter().collect::<Vec<_>>();
                if let Some(extra) = extra {
                    if !wheel
                        .metadata
                        .provides_extra
                        .iter()
                        .any(|provided| provided.parse::<ExtraName>().ok().as_ref() == Some(extra))
                    {
                        return Ok(Dependencies::Unavailable(format!(
                            "extra {extra} is not provided",
                        )));
                    }
                    constraints.insert(
                        Package::Distribution(name.clone(), None),
                        Ranges::singleton(version.clone()),
                    );
                }
                let requirements = wheel
                    .metadata
                    .requires_dist
                    .iter()
                    .map(|requirement| {
                        parse_requirement(requirement)
                            .map_err(|error| Needed::Invalid(error.to_string()))
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                self.constraints(&requirements, &extras, &mut constraints)?;
            }
        }
        Ok(Dependencies::Available(DependencyConstraints::from_iter(constraints)))
    }
}

impl Provider<'_> {
    fn constraints(
        &self,
        requirements: &[Requirement],
        extras: &[ExtraName],
        constraints: &mut BTreeMap<Package, Ranges<Version>>,
    ) -> std::result::Result<(), Needed> {
        for requirement in requirements {
            if !requirement.marker.evaluate(self.environment, extras) {
                continue;
            }
            let candidates = self
                .registry
                .candidates
                .get(&requirement.name)
                .ok_or_else(|| Needed::Index(requirement.name.clone()))?;
            let specifiers = match &requirement.version_or_url {
                Some(VersionOrUrl::VersionSpecifier(specifiers)) => Some(specifiers),
                None => None,
                Some(VersionOrUrl::Url(_)) => {
                    return Err(Needed::Invalid(
                        "Python URL requirements are not supported".to_string(),
                    ));
                }
            };
            let matched = candidates
                .keys()
                .filter(|version| specifiers.is_none_or(|specifiers| specifiers.contains(version)))
                .collect::<Vec<_>>();
            let allow_prerelease = specifiers.is_some_and(|specifiers| {
                specifiers.iter().any(pep440_rs::VersionSpecifier::any_prerelease)
            }) || matched.iter().all(|version| version.any_prerelease());
            let range = matched
                .into_iter()
                .filter(|version| allow_prerelease || !version.any_prerelease())
                .fold(Ranges::empty(), |range, version| {
                    range.union(&Ranges::singleton(version.clone()))
                });
            for extra in std::iter::once(None).chain(requirement.extras.iter().cloned().map(Some)) {
                let package = Package::Distribution(requirement.name.clone(), extra);
                constraints
                    .entry(package)
                    .and_modify(|existing| *existing = existing.intersection(&range))
                    .or_insert_with(|| range.clone());
            }
        }
        Ok(())
    }
}

pub(super) async fn resolve<Reporter: InstallReporter + 'static>(
    registry: &mut Registry<'_>,
    requirements: &[Requirement],
) -> Result<BTreeMap<PackageName, Version>> {
    loop {
        let provider =
            Provider { registry, requirements, environment: &registry.interpreter.environment };
        let needed = match pubgrub::resolve(&provider, Package::Root, Version::new([0])) {
            Ok(solution) => {
                return Ok(solution
                    .into_iter()
                    .filter_map(|(package, version)| match package {
                        Package::Distribution(name, None) => Some((name, version)),
                        _ => None,
                    })
                    .collect());
            }
            Err(
                PubGrubError::ErrorRetrievingDependencies { source, .. }
                | PubGrubError::ErrorChoosingVersion { source, .. }
                | PubGrubError::ErrorInShouldCancel(source),
            ) => source,
            Err(PubGrubError::NoSolution(tree)) => {
                bail!("Python dependency resolution failed:\n{}", report_no_solution(tree));
            }
        };
        match needed {
            Needed::Index(name) => registry.fetch_index(&name).await?,
            Needed::Wheel(name, version) => {
                registry.fetch_wheel::<Reporter>(&name, &version).await?;
            }
            Needed::Invalid(message) => bail!("{message}"),
        }
        tokio::task::yield_now().await;
    }
}

pub(super) fn validate_locked(registry: &Registry<'_>, requirements: &[Requirement]) -> Result<()> {
    let solution = locked_solution(registry, requirements)?;
    if solution.len() != registry.candidates.len() {
        bail!("Python lockfile contains packages outside the dependency graph");
    }
    Ok(())
}

pub(super) fn locked_solution(
    registry: &Registry<'_>,
    requirements: &[Requirement],
) -> Result<BTreeMap<PackageName, Version>> {
    let provider =
        Provider { registry, requirements, environment: &registry.interpreter.environment };
    match pubgrub::resolve(&provider, Package::Root, Version::new([0])) {
        Ok(solution) => Ok(solution
            .into_iter()
            .filter_map(|(package, version)| match package {
                Package::Distribution(name, None) => Some((name, version)),
                _ => None,
            })
            .collect()),
        Err(PubGrubError::NoSolution(tree)) => {
            bail!("Python lockfile does not satisfy the project:\n{}", report_no_solution(tree));
        }
        Err(error) => bail!("Python lockfile does not satisfy the project: {error:?}"),
    }
}

fn report_no_solution(mut tree: DerivationTree<Package, Ranges<Version>, String>) -> String {
    tree.collapse_no_versions();
    DefaultStringReporter::report(&tree)
}
