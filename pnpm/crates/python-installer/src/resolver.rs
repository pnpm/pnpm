use super::registry::Registry;
use miette::Result;
use pep440_rs::Version;
use pep508_rs::{PackageName, Requirement};
use pnpm_python_resolver::Step;
use pnpm_reporter::Reporter as InstallReporter;
use std::collections::BTreeMap;

/// Resolve a project by feeding the resolver what it asks for: an index
/// page for a distribution it has not seen, or a wheel whose metadata it
/// needs, until the project is solved.
pub(super) async fn resolve<Reporter: InstallReporter + 'static>(
    registry: &mut Registry<'_>,
    requirements: &[Requirement],
) -> Result<BTreeMap<PackageName, Version>> {
    loop {
        let environment = registry.interpreter.target.environment.clone();
        match pnpm_python_resolver::step(&registry.packages, requirements, &environment)? {
            Step::Solved(solution) => return Ok(solution),
            Step::NeedCandidates(name) => registry.fetch_index(&name).await?,
            Step::NeedMetadata(name, version) => {
                registry.fetch_wheel::<Reporter>(&name, &version).await?;
            }
        }
        tokio::task::yield_now().await;
    }
}

pub(super) fn validate_locked(registry: &Registry<'_>, requirements: &[Requirement]) -> Result<()> {
    pnpm_python_resolver::validate_locked(
        &registry.packages,
        requirements,
        &registry.interpreter.target.environment,
    )
}

pub(super) fn locked_solution(
    registry: &Registry<'_>,
    requirements: &[Requirement],
) -> Result<BTreeMap<PackageName, Version>> {
    pnpm_python_resolver::locked_solution(
        &registry.packages,
        requirements,
        &registry.interpreter.target.environment,
    )
}
