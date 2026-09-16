pub use add::{AddOptions, plan_add};
pub use manifest::DependencySelection;

mod add;
mod build;
mod cache;
mod environment;
mod generation;
mod host;
mod interpreter;
mod lockfile;
mod manifest;
mod registry;
mod requirements;
mod resolver;
mod sources;
mod targets;
mod workspace;

use base64::{Engine as _, engine::general_purpose::STANDARD};
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
use std::{
    collections::BTreeMap,
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
    root: PathBuf,
    lock: String,
    environment: Option<tempfile::TempDir>,
    previous_environment: Option<Option<PathBuf>>,
}

/// Prepare the selected project manifests as one participant in an install plan.
/// Environment publication and rollback are managed by the install coordinator.
pub fn plan<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    manifests: Vec<PathBuf>,
    selection: DependencySelection,
) -> pnpm_install_coordinator::InstallTask<'static> {
    let metadata = manifests
        .iter()
        .map(|path| path.with_file_name("pylock.toml"))
        .collect();
    pnpm_install_coordinator::InstallTask::new(
        metadata,
        prepare::<Reporter>(context, manifests, false, selection),
    )
}

/// The manifests a run prepares from, or `None` when none of them
/// declares a project. A manifest that only declares a workspace is read
/// for what it says about the others, so it is not by itself a reason to
/// start an interpreter.
async fn discovered_projects(
    manifests: Vec<PathBuf>,
    allowed_root: Option<&Path>,
) -> Result<Option<Vec<(PathBuf, Arc<manifest::Manifest>)>>> {
    let roots = read_project_manifests(manifests, allowed_root).await?
        .into_iter()
        .map(|(root, manifest)| (root, Arc::new(manifest)))
        .collect::<Vec<_>>();
    Ok(roots
        .iter()
        .any(|(_, manifest)| manifest.project.is_some())
        .then_some(roots))
}

async fn prepare<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    manifests: Vec<PathBuf>,
    resolve: bool,
    selection: manifest::DependencySelection,
) -> Result<Vec<Prepared>> {
    let config = context.config;
    let Some(roots) = discovered_projects(manifests, config.workspace_dir.as_deref()).await? else {
        return Ok(Vec::new());
    };
    // A manifest that declares only a workspace is still what says which
    // projects that workspace contains and where they come from.
    let workspace = workspace::Workspace::new(&roots)?;
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
        build_environments: tokio::sync::Mutex::default(),
    };
    let result = prepare_projects::<Reporter>(&shared, workspace, roots).await;
    drop(writer);
    writer_task.await
        .into_diagnostic()
        .wrap_err("join Python artifact store index writer")?
        .into_diagnostic()
        .wrap_err("flush Python artifact store index")?;
    result
}

async fn prepare_projects<Reporter: self::Reporter + 'static>(
    shared: &Shared<'_>,
    mut workspace: workspace::Workspace,
    mut discovered: Vec<(PathBuf, Arc<manifest::Manifest>)>,
) -> Result<Vec<Prepared>> {
    let config = shared.context.config;
    let mut interpreters = Interpreters::new(config);
    let mut selected =
        shared.prepare_metadata::<Reporter>(&mut discovered, &mut interpreters).await?;
    workspace.update_manifests(&discovered);
    let mut prepared = Vec::new();
    for (root, manifest) in discovered {
        if manifest.project.is_none() {
            continue;
        }
        let interpreter = match selected.remove(&root) {
            Some(interpreter) => interpreter,
            None => interpreters.select::<Reporter>(&root, &manifest).await?,
        };
        let environments = Environments::of(config, &interpreter)?;
        workspace.for_resolution(config, &environments);
        let local = Arc::from(workspace.local_projects(&root, &manifest, &root)?);
        prepared.push(
            PythonPrepare::for_project(shared, &interpreter, &environments)
                .project::<Reporter>(root, manifest, local, &workspace)
                .await?,
        );
    }
    Ok(prepared)
}

/// Every `pyproject.toml` among `manifests`, parsed. One without a
/// `[project]` table declares no package of its own, but may still say
/// which projects a workspace contains and where they come from.
async fn read_project_manifests(
    manifests: Vec<PathBuf>,
    allowed_root: Option<&Path>,
) -> Result<Vec<(PathBuf, manifest::Manifest)>> {
    let mut roots = Vec::new();
    for path in manifests {
        let contents = if path
            .file_name()
            .is_some_and(|name| name == "requirements.txt")
        {
            String::new()
        } else {
            tokio::fs::read_to_string(&path).await
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", path.display()))?
        };
        let allowed_root =
            allowed_root.unwrap_or_else(|| path.parent().expect("manifest has a parent"));
        let manifest = read_manifest(&path, &contents, allowed_root).await?;
        if manifest.project.is_some() || manifest.tool.uv.workspace.is_some() {
            roots.push((
                path.parent()
                    .expect("manifest has a parent")
                    .to_path_buf(),
                manifest,
            ));
        }
    }
    Ok(roots)
}

async fn read_manifest(
    path: &Path,
    contents: &str,
    allowed_root: &Path,
) -> Result<manifest::Manifest> {
    let mut manifest = if path
        .file_name()
        .is_some_and(|name| name == "requirements.txt")
    {
        manifest::Manifest::parse("")?
    } else {
        manifest::Manifest::parse(contents)?
    };
    if manifest.project.is_none() {
        let requirements_path = path.with_file_name("requirements.txt");
        if tokio::fs::try_exists(&requirements_path).await.into_diagnostic()? {
            let allowed_root = allowed_root.to_path_buf();
            let requirements = tokio::task::spawn_blocking(move || {
                requirements::read(&requirements_path, &allowed_root)
            })
            .await
            .into_diagnostic()
            .wrap_err("join Python requirements parsing")??;
            manifest.set_requirements_file(requirements);
        }
    }
    Ok(manifest)
}

/// The Python index a project resolves against. Credentials the
/// configured URL carried are lifted into `auth`, so `url` never holds
/// any: it is cached under, and locked as, what it reads.
pub(crate) struct Index {
    pub(crate) url: url::Url,
    pub(crate) auth: pnpm_network::AuthHeaders,
}

/// The configured Python index, with any credentials it carries lifted out
/// of the URL. A repository-selected Python index must not select
/// user-level npm credentials.
fn python_index(config: &pnpm_config::Config) -> Result<Index> {
    let mut index: url::Url = config.python.index_url.parse().into_diagnostic()?;
    let mut auth = pnpm_network::AuthHeaders::default().with_secure_transport();
    if !index.username().is_empty() || index.password().is_some() {
        let username = pnpm_network::percent_decode_str(index.username());
        let password = pnpm_network::percent_decode_str(index.password().unwrap_or(""));
        index
            .set_username("")
            .map_err(|()| miette::miette!("invalid Python index URL"))?;
        index
            .set_password(None)
            .map_err(|()| miette::miette!("invalid Python index URL"))?;
        auth.insert_url_header(
            index.as_str(),
            format!("Basic {}", STANDARD.encode(format!("{username}:{password}"))),
        );
    }
    pnpm_python_resolver::validate_url(&index)?;
    if !index.path().ends_with('/') {
        index.set_path(&format!("{}/", index.path()));
    }
    Ok(Index { url: index, auth })
}

impl PythonPrepare<'_> {
    async fn project<Reporter: self::Reporter + 'static>(
        &self,
        root: PathBuf,
        manifest: Arc<manifest::Manifest>,
        local: Arc<[workspace::LocalProject]>,
        workspace: &workspace::Workspace,
    ) -> Result<Prepared> {
        let project = manifest.project.as_ref().expect("only project manifests were selected");
        self.check_requires_python(&root, project.requires_python.as_deref())?;
        let requirements = manifest.selected_requirements(self.context.config)?;
        let all_requirements = workspace.requirements(&root, &manifest, requirements.all.clone())?;
        let inputs = self.inputs(&all_requirements);
        let mut registry = self.registry();
        workspace::offer(&mut registry.resolution.packages, &local);
        let lock_path = root.join("pylock.toml");
        let lock = self.project_lock::<Reporter>(
            &mut registry,
            LockfileInputs {
                existing: None,
                lock_path: &lock_path,
                requirements: &all_requirements,
                inputs,
                requires_python: project.requires_python.clone(),
                local: Arc::clone(&local),
            },
        )
        .await?;
        let environment = self.environment::<Reporter>(
            &mut registry,
            EnvironmentProject { root: &root, manifest: &manifest, local: &local },
            &lock,
            &workspace.requirements(
                &root,
                &manifest,
                requirements.selected(self.asked.selection).to_vec(),
            )?,
        )
        .await?;
        Ok(Prepared {
            root,
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
        self.lockfile::<Reporter>(registry, inputs).await
    }

    /// What this install's resolution depends on, which is what decides
    /// whether the lockfile on disk still answers it.
    fn inputs(&self, requirements: &[pep508_rs::Requirement]) -> Inputs {
        if self.environments.declared {
            Inputs::declared(
                requirements,
                &self.environments.platforms,
                &self.environments.python_versions,
                self.index.url.as_str(),
            )
        } else {
            Inputs::new(requirements, &self.interpreter.target, self.index.url.as_str())
        }
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
    let mut paths = vec![dir.join(if cfg!(windows) { ".venv/Scripts" } else { ".venv/bin" })];
    paths.extend(config.extra_bin_paths.iter().cloned());
    std::borrow::Cow::Owned(paths)
}

#[cfg(test)]
mod tests;
