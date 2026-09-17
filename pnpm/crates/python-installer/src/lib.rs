pub use add::{AddOptions, plan_add, writable_project};
pub use discovery::{Discovery, PythonProject, discover};
pub use manifest::DependencySelection;
pub use workspace::members::in_declared_workspace;

mod add;
mod build;
mod cache;
mod discovery;
mod environment;
mod generation;
mod host;
mod interpreter;
mod lockfile;
mod manifest;
mod projects;
mod registry;
mod requirements;
mod resolutions;
mod resolver;
mod settings;
mod sources;
mod targets;
mod workspace;

use environment::{Generation, LockfileInputs, PythonPrepare, Shared, publish_link};
use generation::EnvironmentProject;
use host::Interpreter;
use interpreter::{Interpreters, Mismatch};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_pnpr_client::{PYPI_ECOSYSTEM, PnprClient, PypiResolveOptions};
use pnpm_python_resolver::{Inputs, Lockfile};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_store_dir::{StoreIndex, StoreIndexWriter};
use registry::Registry;
use settings::{Index, python_index};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use targets::Environments;

/// Inputs shared by the Python projects participating in one install plan.
#[derive(Clone)]
pub struct InstallOptions {
    pub config: &'static pnpm_config::Config,
    pub http_client: Arc<pnpm_network::ThrottledClient>,
    pub lockfile_only: bool,
    pub frozen_lockfile: bool,
}

struct Prepared {
    /// Where the lockfile and the environment are published.
    root: PathBuf,
    /// The projects the lockfile answers for: `root` alone, or the
    /// members sharing the environment at it.
    members: Vec<PathBuf>,
    lock: String,
    environment: Option<Generation>,
    previous_environment: Option<Option<PathBuf>>,
}

/// Prepare the selected project manifests as one participant in an install plan.
/// Environment publication and rollback are managed by the install coordinator.
pub fn plan<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    discovery: Discovery,
    selection: DependencySelection,
    selected: BTreeSet<PathBuf>,
) -> pnpm_install_coordinator::InstallTask<'static> {
    let metadata = discovery.workspace
        .memberships(&selected)
        .iter()
        .map(|membership| membership.root.join("pylock.toml"))
        .collect();
    pnpm_install_coordinator::InstallTask::new(
        metadata,
        prepare::<Reporter>(context, discovery, false, selection, selected),
    )
}

/// Say which of the platforms this install prepares for pnpm cannot
/// resolve Python for.
///
/// `supportedArchitectures` is read by the npm side too, where a system
/// pnpm has no wheel tags for is still a name a package declares, so one
/// is not an error. It does mean `pylock.toml` covers less than the
/// workspace says it supports, and saying nothing about that reads as
/// having covered it.
fn report_platforms_without_python<Reporter: self::Reporter + 'static>(
    config: &pnpm_config::Config,
) {
    let Some(supported) = config.supported_architectures.as_ref() else {
        return;
    };
    let unnamed = targets::platform_values_without_python(supported);
    if unnamed.is_empty() {
        return;
    }
    let names = unnamed.join(", ");
    let message = if targets::declares_platforms(config) {
        format!("pnpm cannot resolve Python for {names}, so pylock.toml does not cover it.")
    } else {
        format!(
            "supportedArchitectures names no platform pnpm can resolve Python for: {names}. \
             pylock.toml is resolved for the platform the install runs on.",
        )
    };
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
}

async fn prepare<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    discovery: Discovery,
    resolve: bool,
    selection: manifest::DependencySelection,
    selected: BTreeSet<PathBuf>,
) -> Result<Vec<Prepared>> {
    let config = context.config;
    if !discovery.has_projects() {
        return Ok(Vec::new());
    }
    report_platforms_without_python::<Reporter>(config);
    let Discovery { roots, workspace, .. } = discovery;
    let index = python_index(config)?;
    config.store_dir.init().into_diagnostic()?;
    let store_index = StoreIndex::shared_for(&config.store_dir, config.frozen_store);
    let (writer, writer_task) = StoreIndexWriter::spawn_for(&config.store_dir, config.frozen_store);
    let shared = Shared {
        context: &context,
        index: &index,
        store: environment::ArtifactStore { index: store_index, writer: &writer },
        asked: environment::Asked { resolve, selection },
        members: workspace.scopes().clone(),
        caches: environment::Caches::default(),
    };
    let result = projects::prepare::<Reporter>(&shared, workspace, roots, &selected).await;
    drop(writer);
    writer_task.await
        .into_diagnostic()
        .wrap_err("join Python artifact store index writer")?
        .into_diagnostic()
        .wrap_err("flush Python artifact store index")?;
    result
}

impl PythonPrepare<'_> {
    async fn projects<Reporter: self::Reporter + 'static>(
        &self,
        projects: projects::Projects,
    ) -> Result<Prepared> {
        let recorded_members = projects.recorded_members();
        let projects::Projects { root, members, local, rules, .. } = projects;
        for member in &members {
            self.check_requires_python(&member.root, member.requires_python())?;
        }
        let requirements = projects::Requirements::merged(&members);
        let (mut registry, mut inputs) = self.configured_registry(&requirements.all, &rules)?;
        inputs.set_members(recorded_members);
        workspace::offer(&mut registry.resolution.packages, &local);
        let lock_path = root.join("pylock.toml");
        let lock = self.project_lock::<Reporter>(
            &mut registry,
            LockfileInputs {
                existing: None,
                lock_path: &lock_path,
                requirements: &requirements.all,
                inputs,
                requires_python: projects::requires_python_of(&members)?,
                local: Arc::clone(&local),
                members: &members,
            },
        )
        .await?;
        let environment = self.environment::<Reporter>(
            &mut registry,
            EnvironmentProject { root: &root, members: &members, local: &local },
            &lock,
            &requirements.selected,
        )
        .await?;
        Ok(Prepared {
            root,
            members: members
                .into_iter()
                .map(|member| member.root)
                .collect(),
            lock: toml::to_string_pretty(&lock).into_diagnostic()?,
            environment,
            previous_environment: None,
        })
    }

    async fn project_lock<Reporter: self::Reporter + 'static>(
        &self,
        registry: &mut Registry<'_>,
        mut inputs: LockfileInputs<'_>,
    ) -> Result<Lockfile> {
        inputs.existing = self.replayable_lockfile(
            inputs.lock_path,
            &inputs.inputs,
            inputs.requires_python.as_deref(),
            &inputs.local,
        )
        .await?;
        self.shared_lockfile::<Reporter>(registry, inputs).await
    }

    fn registry(&self) -> Registry<'_> {
        Registry {
            config: self.context.config,
            client: &self.context.http_client,
            index: self.index,
            interpreter: self.interpreter,
            resolution: registry::Resolution::new(self.interpreter.target.clone()),
            store: pnpm_tarball::ArchiveStoreContext {
                dir: &self.context.config.store_dir,
                index: self.store.index.clone(),
                index_writer: Some(Arc::clone(self.store.writer)),
                verify_integrity: self.context.config.verify_store_integrity,
                strict_pkg_content_check: self.context.config.strict_store_pkg_content_check,
                verified_files_cache: Arc::default(),
                prefetched_cas_paths: None,
            },
            wheels: BTreeMap::new(),
            sources: sources::Sources::new(self),
        }
    }

    /// A project that pins an interpreter range cannot be locked for an
    /// environment outside it, which for a project that declares none is
    /// the interpreter running the install.
    ///
    /// A declared environment outside the range is a configuration the
    /// resolution cannot satisfy, so it is reported whatever
    /// `runtimeOnFail` says. The undeclared environment is the selected
    /// interpreter, which the selection has already reported on under
    /// that setting.
    fn check_requires_python(&self, root: &Path, requires_python: Option<&str>) -> Result<()> {
        if !self.environments.declared && Mismatch::of(self.context.config).bypassed() {
            return Ok(());
        }
        let Some(specifiers) = requires_python else {
            return Ok(());
        };
        let specifiers: pep440_rs::VersionSpecifiers = specifiers.parse().into_diagnostic()?;
        for environment in &self.environments.list {
            let version = environment.target.environment.python_full_version();
            if !specifiers.contains(version) {
                bail!(
                    "{} requires Python {specifiers}, but {version} was selected",
                    root.display(),
                );
            }
        }
        Ok(())
    }
}

impl pnpm_install_coordinator::PreparedInstall for Prepared {
    fn publish(&mut self) -> Result<()> {
        if let Some(generation) = &self.environment {
            self.previous_environment = Some(generation.store.validate_link(&self.root)?);
            publish_link(&self.root, generation.directory.path())?;
        }
        let lock_path = self.root.join("pylock.toml");
        let previous = match fs::read_to_string(&lock_path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).into_diagnostic(),
        };
        if previous.as_deref() != Some(&self.lock) {
            pnpm_fs::write_atomic(&lock_path, self.lock.as_bytes()).into_diagnostic()?;
        }
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        match &self.previous_environment {
            Some(Some(previous)) => publish_link(&self.root, previous),
            Some(None) => match pnpm_fs::remove_symlink_dir(&self.root.join(".venv")) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error).into_diagnostic(),
            },
            None => Ok(()),
        }
    }

    fn retain(self: Box<Self>) {
        if let Some(generation) = self.environment {
            let _ = generation.directory.keep();
        }
    }
}

pub fn execution_paths<'a>(
    config: &'a pnpm_config::Config,
    dir: &Path,
) -> std::borrow::Cow<'a, [PathBuf]> {
    if !config.python.enabled {
        return std::borrow::Cow::Borrowed(&config.extra_bin_paths);
    }
    let environment = environment_dir(config.workspace_dir.as_deref(), dir);
    let mut paths = vec![environment.join(if cfg!(windows) { "Scripts" } else { "bin" })];
    paths.extend(config.extra_bin_paths.iter().cloned());
    std::borrow::Cow::Owned(paths)
}

fn environment_dir(workspace: Option<&Path>, dir: &Path) -> PathBuf {
    workspace::members::environment_root_of(workspace, dir).join(".venv")
}

#[cfg(test)]
mod tests;
