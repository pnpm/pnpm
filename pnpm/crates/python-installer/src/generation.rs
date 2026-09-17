//! A fresh environment generation, and what it installs: the wheels the
//! lockfile pins and the projects in this repository built from source.

use super::{
    Lockfile, Registry, Reporter, build,
    environment::{PythonPrepare, ensure_environment_parent, validate_environment_link},
    host, manifest, resolver, workspace,
};
use miette::{IntoDiagnostic, Result, bail};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
use std::{collections::BTreeMap, path::Path};

impl PythonPrepare<'_> {
    /// Build every project in this repository the solution selected.
    async fn build_solved<Reporter: self::Reporter + 'static>(
        &self,
        solution: &BTreeMap<pep508_rs::PackageName, pep440_rs::Version>,
        local: &[workspace::LocalProject],
    ) -> Result<BTreeMap<pep508_rs::PackageName, build::Build>> {
        let mut selected = Vec::new();
        for project in local {
            if solution.get(&project.name) == Some(&project.version) {
                selected.push(project);
            }
        }
        self.build_projects::<Reporter>(&selected).await
    }

    /// Install what the project selects into a fresh environment
    /// generation: the locked wheels, the projects in this repository it
    /// depends on, and the project itself when it builds a package. A
    /// lockfile-only run builds none of it.
    pub(super) async fn environment<Reporter: self::Reporter + 'static>(
        &self,
        registry: &mut Registry<'_>,
        project: EnvironmentProject<'_>,
        lock: &Lockfile,
        selected: &[pep508_rs::Requirement],
    ) -> Result<Option<tempfile::TempDir>> {
        if self.context.lockfile_only {
            return Ok(None);
        }
        let environment = new_generation(project.root)?;
        // The lockfile may cover several environments; this one installs
        // for the interpreter that is running.
        registry.resolution.answer_for(self.interpreter.target.clone());
        registry.resolution.packages.candidates.clear();
        lock.seed(&mut registry.resolution.packages, &self.interpreter.target)?;
        workspace::offer_locked(&mut registry.resolution.packages, project.local, lock);
        registry.record_sources(selected)?;
        registry.fetch_wheels::<Reporter>(selected).await?;
        let solution = resolver::selected_solution(&mut registry.resolution, selected)?;
        let installed = self.installable::<Reporter>(registry, solution, project).await?;
        let packages = installed.packages;
        host::install(
            &self.interpreter.executable,
            environment.path(),
            packages,
            self.context.config.python.link_mode,
        )
        .await?;
        drop(installed.unpacked);
        Ok(Some(environment))
    }

    /// The wheels an environment installs: the ones the lockfile pins, and
    /// the ones built from the projects in this repository it selects.
    async fn installable<Reporter: self::Reporter + 'static>(
        &self,
        registry: &Registry<'_>,
        solution: BTreeMap<pep508_rs::PackageName, pep440_rs::Version>,
        project: EnvironmentProject<'_>,
    ) -> Result<Installable> {
        let EnvironmentProject { root, manifest, local } = project;
        let mut built = self.build_solved::<Reporter>(&solution, local).await?;
        let mut installable = Installable::default();
        let mut unapproved = BTreeMap::new();
        for package in solution {
            match built.remove(&package.0) {
                Some(build::Build::Made(build)) => installable.made(*build),
                Some(build::Build::NotApproved(names)) => {
                    unapproved.insert(package.0, names);
                }
                None => {
                    let mut wheel = registry.wheels[&package].clone();
                    if let Some(provenance) = registry.source_provenance(&package.0) {
                        wheel.direct_url = Some(provenance);
                    }
                    installable.packages.push(wheel);
                }
            }
        }
        if manifest.is_packaged() {
            match self.build_self::<Reporter>(root, manifest).await? {
                build::Build::Made(build) => installable.made(*build),
                build::Build::NotApproved(names) => {
                    let name = manifest
                        .distribution()
                        .cloned()
                        .expect("a packaged project declares a distribution");
                    unapproved.insert(name, names);
                }
            }
        }
        report_unapproved_builds::<Reporter>(self.context.config, &unapproved)?;
        Ok(installable)
    }
}

/// Report the projects pnpm did not build because nothing has approved
/// their backends to run.
///
/// A backend is code from the index that a build executes, so it is
/// approved the way a dependency's build scripts are, and an install that
/// silently left a project out would be one whose environment cannot run
/// it.
fn report_unapproved_builds<Reporter: self::Reporter + 'static>(
    config: &pnpm_config::Config,
    unapproved: &BTreeMap<pep508_rs::PackageName, Vec<String>>,
) -> Result<()> {
    if unapproved.is_empty() {
        return Ok(());
    }
    let mut backends = unapproved
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    backends.sort();
    backends.dedup();
    let projects = unapproved
        .keys()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let backend = if backends.len() == 1 { "build requirement" } else { "build requirements" };
    let names = backends.join(", ");
    let message = format!(
        "Not building the Python {} {projects}, because the {backend} {names} \
         {} not approved to run. Add {} under allowBuilds in pnpm-workspace.yaml.",
        if unapproved.len() == 1 { "project" } else { "projects" },
        if backends.len() == 1 { "is" } else { "are" },
        if backends.len() == 1 { "it" } else { "them" },
    );
    if config.strict_dep_builds {
        bail!("{message}");
    }
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
    Ok(())
}

/// A fresh environment generation beside the project, which publication
/// makes the project's own once every participant has prepared.
fn new_generation(root: &Path) -> Result<tempfile::TempDir> {
    validate_environment_link(root)?;
    ensure_environment_parent(root)?;
    tempfile::Builder::new()
        .prefix("env-")
        .tempdir_in(root.join(".pnpm/python-envs"))
        .into_diagnostic()
}

/// What an environment installs, and the directories the built wheels
/// were unpacked into, which are removed once it has.
#[derive(Default)]
struct Installable {
    packages: Vec<host::Wheel>,
    unpacked: Vec<std::sync::Arc<tempfile::TempDir>>,
}

impl Installable {
    fn made(&mut self, build: build::Built) {
        self.packages.push(build.wheel);
        self.unpacked.push(build.output);
    }
}

/// The project an environment is being built for.
pub(super) struct EnvironmentProject<'a> {
    pub(super) root: &'a Path,
    pub(super) manifest: &'a manifest::Manifest,
    pub(super) local: &'a [workspace::LocalProject],
}
