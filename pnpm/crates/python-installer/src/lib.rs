pub use add::{AddOptions, plan_add, writable_project};
pub use discovery::{Discovery, PythonProject, discover};
pub use manifest::DependencySelection;

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

use environment::{LockfileInputs, PythonPrepare, Shared, publish_link, validate_environment_link};
use generation::EnvironmentProject;
use host::Interpreter;
use interpreter::Interpreters;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_pnpr_client::{PYPI_ECOSYSTEM, PnprClient, PypiResolveOptions};
use pnpm_python_resolver::{Inputs, Lockfile};
use pnpm_reporter::Reporter;
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
    environment: Option<tempfile::TempDir>,
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
    fn check_requires_python(&self, root: &Path, requires_python: Option<&str>) -> Result<()> {
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
        if let Some(environment) = &self.environment {
            self.previous_environment = Some(validate_environment_link(&self.root)?);
            publish_link(&self.root, environment.path())?;
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
        if let Some(environment) = self.environment {
            let _ = environment.keep();
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

/// The environment a command run in `dir` uses: the one at the workspace
/// root when `dir` is a member of a workspace sharing it, and its own
/// otherwise, which is where an install would create it.
fn environment_dir(workspace: Option<&Path>, dir: &Path) -> PathBuf {
    workspace::members::shared_root_of(workspace, dir)
        .unwrap_or_else(|| dir.to_path_buf())
        .join(".venv")
}

#[cfg(test)]
mod tests;
