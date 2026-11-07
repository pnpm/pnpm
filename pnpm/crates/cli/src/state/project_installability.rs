//! The check of what a project's own manifest requires of the host: its
//! `engines.node`, `engines.pnpm`, `os`, `cpu`, and `libc`.
//!
//! - An unsatisfied `engines.pnpm` fails.
//! - An unsatisfied `engines.node` fails under `engineStrict` and is a
//!   warning otherwise.
//! - An unsupported platform is a warning.

use super::InitStateError;
use pnpm_config::{Config, PNPM_VERSION};
use pnpm_package_is_installable::{
    InstallabilityError, InstallabilityOptions, PackageInstallabilityManifest, check_package,
};
use pnpm_package_manifest::node_version_from_engines_runtime;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
use pnpm_workspace::Project;
use std::{
    cell::OnceCell,
    path::{Path, PathBuf},
};

/// The host the projects of one install are checked against.
pub(crate) struct ProjectHost<'a> {
    config: &'a Config,
    root_dir: &'a Path,
    /// Detected for the first project that declares `engines.node`.
    node_version: OnceCell<String>,
}

impl<'a> ProjectHost<'a> {
    /// `root_dir` holds the manifest whose `engines.runtime` sets the
    /// Node.js version when runtimes are not skipped and no `nodeVersion` is
    /// configured.
    pub(crate) fn new(config: &'a Config, root_dir: &'a Path) -> Self {
        ProjectHost { config, root_dir, node_version: OnceCell::new() }
    }

    fn node_version(&self) -> &str {
        self.node_version.get_or_init(|| {
            let configured =
                self.config.node_version.clone().or_else(|| self.root_runtime_node_version());
            pnpm_deps_restorer::InstallabilityHost::detect_with(
                self.config.engine_strict,
                configured,
            )
            .node_version
        })
    }

    fn root_runtime_node_version(&self) -> Option<String> {
        if self.config.skip_runtimes {
            return None;
        }
        // An unreadable root manifest fails the root project's own check,
        // which every install runs, so the error is reported there.
        let (_, manifest) = pnpm_workspace::try_read_project_manifest(self.root_dir).ok()??;
        node_version_from_engines_runtime(manifest.value())
    }

    /// Check the project in `project_dir`, returning the warnings to emit for
    /// mismatches that do not fail. Its engines and its platform are checked
    /// apart, so a platform mismatch does not hide an engine failure.
    pub(crate) fn check(
        &self,
        project_dir: &Path,
        requirements: &pnpm_deps_restorer::ProjectHostRequirements,
    ) -> Result<Vec<LogEvent>, InitStateError> {
        let engines = PackageInstallabilityManifest {
            engines: requirements.installability.engines.clone(),
            ..PackageInstallabilityManifest::default()
        };
        let platform =
            PackageInstallabilityManifest { engines: None, ..requirements.installability.clone() };
        [engines, platform]
            .iter()
            .filter_map(|part| self.check_part(project_dir, requirements, part).transpose())
            .collect()
    }

    fn check_part(
        &self,
        project_dir: &Path,
        host_requirements: &pnpm_deps_restorer::ProjectHostRequirements,
        requirements: &PackageInstallabilityManifest,
    ) -> Result<Option<LogEvent>, InitStateError> {
        let wanted_engine = requirements.engines.as_ref();
        let project_node_version = self.config.node_version.is_none()
            && !self.config.skip_runtimes
            && wanted_engine.and_then(|engine| engine.node.as_ref()).is_some();
        let node_version = match wanted_engine.and_then(|engine| engine.node.as_ref()) {
            Some(_) if project_node_version => {
                host_requirements.node_version.as_deref().unwrap_or_else(|| self.node_version())
            }
            Some(_) => self.node_version(),
            None => "",
        };
        let pnpm_version =
            wanted_engine.and_then(|engine| engine.pnpm.as_ref()).map(|_| PNPM_VERSION);
        let options = InstallabilityOptions {
            engine_strict: self.config.engine_strict,
            optional: false,
            current_node_version: node_version,
            pnpm_version,
            current_os: pnpm_graph_hasher::host_platform(),
            current_cpu: pnpm_graph_hasher::host_arch(),
            current_libc: pnpm_graph_hasher::host_libc(),
            supported_architectures: None,
        };
        let error = match check_package(&project_dir.to_string_lossy(), requirements, &options) {
            Ok(None) => return Ok(None),
            Ok(Some(error)) => error,
            Err(invalid) => InstallabilityError::InvalidNodeVersion(invalid),
        };
        if fails(&error, self.config.engine_strict) {
            return Err(InitStateError::Installability(Box::new(error)));
        }
        Ok(Some(LogEvent::Global(GlobalLog { level: LogLevel::Warn, message: error.to_string() })))
    }

    /// Check every project in `projects` except the one in the root
    /// directory, which [`super::check_root_project_engine`] checks. Returns
    /// the warnings to emit, so that a run that fails emits none.
    pub(crate) fn check_projects(
        &self,
        projects: &[(PathBuf, pnpm_deps_restorer::ProjectHostRequirements)],
    ) -> Result<Vec<LogEvent>, InitStateError> {
        let root_dir = pnpm_fs::lexical_normalize(self.root_dir);
        self.check_each(
            projects
                .iter()
                .filter(|(project_dir, _)| pnpm_fs::lexical_normalize(project_dir) != root_dir),
        )
    }

    /// [`Self::check_projects`], the project in the root directory included.
    pub(crate) fn check_every_project(
        &self,
        projects: &[(PathBuf, PackageInstallabilityManifest)],
    ) -> Result<Vec<LogEvent>, InitStateError> {
        self.check_each(projects.iter())
    }

    fn check_each<'p>(
        &self,
        projects: impl Iterator<Item = &'p (PathBuf, PackageInstallabilityManifest)>,
    ) -> Result<Vec<LogEvent>, InitStateError> {
        let mut warnings = Vec::new();
        for (project_dir, requirements) in projects {
            warnings.extend(self.check(project_dir, requirements)?);
        }
        Ok(warnings)
    }
}

/// What each of `projects` that declares any requirement requires of the
/// host, keyed by its directory.
pub(crate) fn host_requirements<'p>(
    projects: impl IntoIterator<Item = &'p Project>,
) -> Vec<(PathBuf, PackageInstallabilityManifest)> {
    projects
        .into_iter()
        .filter_map(|project| {
            pnpm_deps_restorer::project_host_requirements(project.manifest.value())
                .map(|requirements| (project.root_dir.clone(), requirements))
        })
        .collect()
}

fn fails(error: &InstallabilityError, engine_strict: bool) -> bool {
    match error {
        InstallabilityError::Engine(error) => engine_strict || error.wanted.pnpm.is_some(),
        InstallabilityError::Platform(_) => false,
        InstallabilityError::InvalidNodeVersion(_) => true,
    }
}
