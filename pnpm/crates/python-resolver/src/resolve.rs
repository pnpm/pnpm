use crate::{metadata::WheelMetadata, packages::Packages};
use miette::{Result, bail};
use pep440_rs::Version;
use pep508_rs::{ExtraName, MarkerEnvironment, PackageName, Requirement, VersionOrUrl};
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

/// What one pubgrub pass produced: the solution, or the one thing the
/// resolution has to learn before it can go on.
#[derive(Debug)]
pub enum Step {
    Solved(BTreeMap<PackageName, Version>),
    /// The versions this index offers of a distribution nothing has read
    /// yet — see [`crate::candidates_from_page`].
    NeedCandidates(PackageName),
    /// The `METADATA` of one wheel — see [`WheelMetadata::parse`].
    NeedMetadata(PackageName, Version),
}

#[derive(Debug)]
enum Needed {
    Candidates(PackageName),
    Metadata(PackageName, Version),
    Invalid(String),
}

impl fmt::Display for Needed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Needed {}

struct Provider<'a> {
    packages: &'a Packages,
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
            self.packages.candidates.get(name).ok_or_else(|| Needed::Candidates(name.clone()))?;
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
                let metadata = self
                    .packages
                    .metadata
                    .get(&(name.clone(), version.clone()))
                    .ok_or_else(|| Needed::Metadata(name.clone(), version.clone()))?;
                if let Some(unusable) = self.incompatible_interpreter(metadata)? {
                    return Ok(Dependencies::Unavailable(unusable));
                }
                let extras = extra.clone().into_iter().collect::<Vec<_>>();
                if let Some(extra) = extra {
                    if !metadata
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
                let requirements = metadata
                    .requires_dist
                    .iter()
                    .map(|requirement| {
                        crate::candidates::parse_requirement(requirement)
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
    /// Why this interpreter cannot use the wheel, when it cannot: a
    /// `Requires-Python` the running interpreter is outside of.
    fn incompatible_interpreter(
        &self,
        metadata: &WheelMetadata,
    ) -> std::result::Result<Option<String>, Needed> {
        let Some(specifier) = &metadata.requires_python else { return Ok(None) };
        let specifier: pep440_rs::VersionSpecifiers = specifier
            .parse()
            .map_err(|error| Needed::Invalid(format!("invalid Requires-Python: {error}")))?;
        Ok((!specifier.contains(self.environment.python_full_version()))
            .then(|| "incompatible Python interpreter".to_string()))
    }

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
                .packages
                .candidates
                .get(&requirement.name)
                .ok_or_else(|| Needed::Candidates(requirement.name.clone()))?;
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

/// Run one pubgrub pass over what `packages` holds so far.
///
/// A resolution is this step called in a loop: it either solves the
/// project or names the one distribution or wheel it still needs, which
/// the caller fetches, records, and steps again. A project that cannot be
/// solved at all fails here with pubgrub's own explanation.
pub fn step(
    packages: &Packages,
    requirements: &[Requirement],
    environment: &MarkerEnvironment,
) -> Result<Step> {
    let provider = Provider { packages, requirements, environment };
    match pubgrub::resolve(&provider, Package::Root, Version::new([0])) {
        Ok(solution) => Ok(Step::Solved(distributions(solution))),
        Err(
            PubGrubError::ErrorRetrievingDependencies { source, .. }
            | PubGrubError::ErrorChoosingVersion { source, .. }
            | PubGrubError::ErrorInShouldCancel(source),
        ) => match source {
            Needed::Candidates(name) => Ok(Step::NeedCandidates(name)),
            Needed::Metadata(name, version) => Ok(Step::NeedMetadata(name, version)),
            Needed::Invalid(message) => bail!("{message}"),
        },
        Err(PubGrubError::NoSolution(tree)) => {
            bail!("Python dependency resolution failed:\n{}", report_no_solution(tree));
        }
    }
}

/// Solve a project against candidates that are already all known — a
/// locked install, where every candidate came from the lockfile. Anything
/// still missing is the lockfile failing to satisfy the project.
pub fn locked_solution(
    packages: &Packages,
    requirements: &[Requirement],
    environment: &MarkerEnvironment,
) -> Result<BTreeMap<PackageName, Version>> {
    let provider = Provider { packages, requirements, environment };
    match pubgrub::resolve(&provider, Package::Root, Version::new([0])) {
        Ok(solution) => Ok(distributions(solution)),
        Err(PubGrubError::NoSolution(tree)) => {
            bail!("Python lockfile does not satisfy the project:\n{}", report_no_solution(tree));
        }
        Err(error) => bail!("Python lockfile does not satisfy the project: {error:?}"),
    }
}

/// Check that a lockfile is exactly the project's dependency graph: it
/// satisfies the requirements, and it carries nothing the graph does not
/// reach.
pub fn validate_locked(
    packages: &Packages,
    requirements: &[Requirement],
    environment: &MarkerEnvironment,
) -> Result<()> {
    let solution = locked_solution(packages, requirements, environment)?;
    if solution.len() != packages.candidates.len() {
        bail!("Python lockfile contains packages outside the dependency graph");
    }
    Ok(())
}

fn distributions(
    solution: pubgrub::SelectedDependencies<Package, Version>,
) -> BTreeMap<PackageName, Version> {
    solution
        .into_iter()
        .filter_map(|(package, version)| match package {
            Package::Distribution(name, None) => Some((name, version)),
            _ => None,
        })
        .collect()
}

fn report_no_solution(mut tree: DerivationTree<Package, Ranges<Version>, String>) -> String {
    tree.collapse_no_versions();
    DefaultStringReporter::report(&tree)
}
