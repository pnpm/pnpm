use super::{
    registry::{Registry, Resolution},
    targets::Environment,
};
use miette::Result;
use pep440_rs::Version;
use pep508_rs::{PackageName, Requirement};
use pnpm_python_resolver::{Solved, Step};
use pnpm_reporter::Reporter as InstallReporter;
use std::collections::BTreeMap;

/// Resolve the project once for every environment it locks for, leaving
/// the registry answering for the interpreter running the install again.
///
/// Each environment gets its own candidates, since the wheel a version
/// offers is the one that environment takes. The metadata is read once
/// for the version and shared: a release declares its requirements in
/// the `pyproject.toml` every one of its wheels is built from, so
/// downloading each environment's wheel to read them again would buy
/// nothing. Two wheels of a version whose `METADATA` disagrees are a
/// broken release, and pnpm reads whichever it downloaded first.
pub(super) async fn resolve_all<Reporter: InstallReporter + 'static>(
    registry: &mut Registry<'_>,
    requirements: &[Requirement],
    environments: &[Environment],
) -> Result<Vec<Solved>> {
    let mut solved = Vec::new();
    for environment in environments {
        registry.resolution.answer_for(environment.target.clone());
        let solution = resolve::<Reporter>(registry, requirements).await?;
        solved.push(Solved::new(
            environment.target.clone(),
            solution,
            &registry.resolution.packages,
            environment.declared.clone(),
        )?);
    }
    registry.resolution.answer_for(registry.interpreter.target.clone());
    Ok(solved)
}

/// Resolve a project by feeding the resolver what it asks for: an index
/// page for a distribution it has not seen, or a wheel whose metadata it
/// needs, until the project is solved.
pub(super) async fn resolve<Reporter: InstallReporter + 'static>(
    registry: &mut Registry<'_>,
    requirements: &[Requirement],
) -> Result<BTreeMap<PackageName, Version>> {
    loop {
        let environment = registry.resolution.target.environment.clone();
        match pnpm_python_resolver::step(&registry.resolution.packages, requirements, &environment)?
        {
            Step::Solved(solution) => return Ok(solution),
            Step::NeedCandidates(name) => registry.fetch_index(&name).await?,
            Step::NeedMetadata(name, version) => {
                registry.fetch_wheel::<Reporter>(&name, &version).await?;
            }
        }
        tokio::task::yield_now().await;
    }
}

pub(super) fn validate_locked(resolution: &Resolution, requirements: &[Requirement]) -> Result<()> {
    pnpm_python_resolver::validate_locked(
        &resolution.packages,
        requirements,
        &resolution.target.environment,
    )
}

pub(super) fn locked_solution(
    resolution: &Resolution,
    requirements: &[Requirement],
) -> Result<BTreeMap<PackageName, Version>> {
    pnpm_python_resolver::locked_solution(
        &resolution.packages,
        requirements,
        &resolution.target.environment,
    )
}
