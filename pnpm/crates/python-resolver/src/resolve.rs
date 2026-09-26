pub use sources::active_locked_sources;

mod explain;
mod rules;
mod sources;

use crate::{
    candidates::{Refusal, read_requirement},
    metadata::WheelMetadata,
    packages::Packages,
    requires_python::declared_range,
};
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
    /// The tentative sources cannot satisfy the selected dependency graph.
    Backtrack(String),
    /// The versions this index offers of a distribution nothing has read
    /// yet — see [`crate::candidates_from_page`].
    NeedCandidates(PackageName),
    /// An explicit source that must win over index candidates.
    NeedUrl(PackageName, String),
    /// The `METADATA` of one wheel — see [`WheelMetadata::parse`].
    NeedMetadata(PackageName, Version),
}

#[derive(Debug)]
enum Needed {
    Candidates(PackageName),
    Url(PackageName, String),
    Metadata(PackageName, Version),
    Invalid(String),
    RejectedSource(String),
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
        Ok(versions
            .keys()
            .rev()
            .find(|version| range.contains(version))
            .cloned())
    }

    fn get_dependencies(
        &self,
        package: &Package,
        version: &Version,
    ) -> std::result::Result<Dependencies<Package, Self::VS, String>, Needed> {
        let mut constraints = BTreeMap::<Package, Ranges<Version>>::new();
        let result = match package {
            Package::Root => self.constraints(self.requirements, &[], &mut constraints),
            Package::Distribution(name, extra) => {
                let metadata = self.packages.metadata
                    .get(&(name.clone(), version.clone()))
                    .ok_or_else(|| Needed::Metadata(name.clone(), version.clone()))?;
                if let Some(unusable) = self.incompatible_interpreter(metadata) {
                    return Ok(Dependencies::Unavailable(unusable));
                }
                let extras = extra
                    .clone()
                    .into_iter()
                    .collect::<Vec<_>>();
                if let Some(extra) = extra {
                    if !provides_extra(metadata, extra) {
                        return Ok(Dependencies::Unavailable(format!(
                            "extra {extra} is not provided",
                        )));
                    }
                    constraints.insert(
                        Package::Distribution(name.clone(), None),
                        Ranges::singleton(version.clone()),
                    );
                }
                let requirements = match metadata_requirements(metadata) {
                    Ok(requirements) => requirements,
                    Err(refusal) => return unusable_release(refusal),
                };
                self.constraints(&requirements, &extras, &mut constraints)
            }
        };
        source_dependencies(result, constraints)
    }
}

fn source_dependencies(
    result: std::result::Result<(), Needed>,
    constraints: BTreeMap<Package, Ranges<Version>>,
) -> std::result::Result<Dependencies<Package, Ranges<Version>, String>, Needed> {
    match result {
        Err(Needed::RejectedSource(message)) => Ok(Dependencies::Unavailable(message)),
        Err(error) => Err(error),
        Ok(()) => Ok(Dependencies::Available(DependencyConstraints::from_iter(constraints))),
    }
}

fn provides_extra(metadata: &WheelMetadata, extra: &ExtraName) -> bool {
    metadata.provides_extra
        .iter()
        .any(|provided| {
            provided
                .parse::<ExtraName>()
                .ok()
                .as_ref()
                == Some(extra)
        })
}

/// What a release whose requirements pnpm cannot use offers the solver: a
/// version to pass over, when one of its requirements is unreadable, and
/// nothing at all when it names a requirement pnpm does not implement,
/// which every release declaring it would name too.
fn unusable_release(
    refusal: Refusal,
) -> std::result::Result<Dependencies<Package, Ranges<Version>, String>, Needed> {
    match refusal {
        Refusal::Unreadable(error) => Ok(Dependencies::Unavailable(format!(
            "because its metadata declares a requirement pnpm cannot read: {error}",
        ))),
        Refusal::Unsupported(requirement) => Err(Needed::Invalid(format!(
            "unsupported scheme in direct URL Python requirement: {requirement}",
        ))),
    }
}

/// The requirements a wheel declares, or the reason pnpm cannot use them.
/// A requirement pnpm does not implement outranks one it cannot read
/// wherever the two appear, so what a release costs a project does not
/// depend on the order its metadata happens to list them in.
fn metadata_requirements(
    metadata: &WheelMetadata,
) -> std::result::Result<Vec<Requirement>, Refusal> {
    let mut requirements = Vec::with_capacity(metadata.requires_dist.len());
    let mut unreadable = None;
    for declared in &metadata.requires_dist {
        match read_requirement(declared) {
            Ok(requirement) => requirements.push(requirement),
            Err(unsupported @ Refusal::Unsupported(_)) => return Err(unsupported),
            Err(refusal) => unreadable = unreadable.or(Some(refusal)),
        }
    }
    unreadable.map_or(Ok(requirements), Err)
}

impl Provider<'_> {
    fn check_source(&self, requirement: &Requirement) -> std::result::Result<(), Needed> {
        let Some(VersionOrUrl::Url(url)) = &requirement.version_or_url else { return Ok(()) };
        let source = url.as_str();
        if self.packages.rejected_sources.contains(&(requirement.name.clone(), source.to_string()))
        {
            return Err(Needed::RejectedSource(format!(
                "unavailable Python source for {}",
                requirement.name,
            )));
        }
        let parsed =
            crate::Source::parse(source).map_err(|error| Needed::Invalid(error.to_string()))?;
        if let Some(chosen) = self.packages.direct_urls.get(&requirement.name) {
            let chosen_source =
                crate::Source::parse(chosen).map_err(|error| Needed::Invalid(error.to_string()))?;
            if !chosen_source.compatible_with(&parsed) {
                return Err(Needed::RejectedSource(format!(
                    "conflicting Python sources for {}",
                    requirement.name,
                )));
            }
        }
        if self.packages.candidates
            .get(&requirement.name)
            .is_some_and(|versions| {
                !versions.is_empty()
                    && versions
                        .values()
                        .all(|candidate| candidate.matches_source(&parsed))
            })
        {
            return Ok(());
        }
        Err(Needed::Url(requirement.name.clone(), source.to_string()))
    }

    /// Why this interpreter cannot use the wheel, when it cannot: a
    /// `Requires-Python` the running interpreter is outside of.
    fn incompatible_interpreter(&self, metadata: &WheelMetadata) -> Option<String> {
        let specifier = metadata.requires_python.as_deref().and_then(declared_range)?;
        (!specifier.contains(self.environment.python_full_version())).then(|| {
            "incompatible Python interpreter".to_string()
        })
    }

    fn constraints(
        &self,
        requirements: &[Requirement],
        extras: &[ExtraName],
        constraints: &mut BTreeMap<Package, Ranges<Version>>,
    ) -> std::result::Result<(), Needed> {
        let is_url = |requirement: &&Requirement| {
            matches!(requirement.version_or_url, Some(VersionOrUrl::Url(_)))
        };
        for requirement in requirements
            .iter()
            .filter(is_url)
            .chain(
                requirements
                    .iter()
                    .filter(|requirement| !is_url(requirement)),
            )
        {
            if !requirement.marker.evaluate(self.environment, extras) {
                continue;
            }
            self.check_source(requirement)?;
            let range = self.requirement_range(requirement, extras)?;
            for extra in std::iter::once(None)
                .chain(
                    self.requirement_extras(requirement, extras)
                        .into_iter()
                        .map(Some),
                )
            {
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
        Ok(solution) => {
            if sources::has_inactive_source(&provider, &solution)? {
                return Ok(Step::Backtrack(
                    "Python dependency resolution selected an inactive source".to_string(),
                ));
            }
            Ok(Step::Solved(distributions(solution)))
        }
        Err(
            PubGrubError::ErrorRetrievingDependencies { source, .. }
            | PubGrubError::ErrorChoosingVersion { source, .. }
            | PubGrubError::ErrorInShouldCancel(source),
        ) => match source {
            Needed::Candidates(name) => Ok(Step::NeedCandidates(name)),
            Needed::Url(name, url) => Ok(Step::NeedUrl(name, url)),
            Needed::Metadata(name, version) => Ok(Step::NeedMetadata(name, version)),
            Needed::Invalid(message) | Needed::RejectedSource(message) => bail!("{message}"),
        },
        Err(PubGrubError::NoSolution(tree)) => {
            let message = format!(
                "Python dependency resolution failed:\n{}",
                report_no_solution(tree, packages),
            );
            if !packages.direct_urls.is_empty() {
                return Ok(Step::Backtrack(message));
            }
            bail!("{message}");
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
        Ok(solution) => {
            if sources::has_inactive_source(&provider, &solution)? {
                bail!("Python lockfile selected an inactive source");
            }
            Ok(distributions(solution))
        }
        Err(PubGrubError::NoSolution(tree)) => {
            bail!(
                "Python lockfile does not satisfy the project:\n{}",
                report_no_solution(tree, packages),
            );
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

/// pubgrub's own explanation, with a line for each distribution it was
/// left with no version of.
fn report_no_solution(
    mut tree: DerivationTree<Package, Ranges<Version>, String>,
    packages: &Packages,
) -> String {
    tree.collapse_no_versions();
    format!(
        "{}{}",
        DefaultStringReporter::report(&tree),
        explain::unoffered_distributions(&tree, packages),
    )
}
