use super::{
    Environments, Interpreters, Prepared,
    environment::{PythonPrepare, Shared},
    manifest, workspace,
};
use futures_util::{StreamExt, stream};
use miette::Result;
use pnpm_reporter::Reporter;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) async fn prepare<Reporter: self::Reporter + 'static>(
    shared: &Shared<'_>,
    workspace: workspace::Workspace,
    discovered: Vec<(PathBuf, Arc<manifest::Manifest>)>,
) -> Result<Vec<Prepared>> {
    let config = shared.context.config;
    let projects = select::<Reporter>(shared, workspace, discovered).await?;
    let results =
        stream::iter(projects.into_iter().enumerate())
            .map(|(position, project)| async move {
                (position, project.prepare::<Reporter>(shared).await)
            })
            .buffer_unordered(config.workspace_concurrency.max(1) as usize)
            .collect::<BTreeMap<_, _>>()
            .await;
    results.into_values().collect()
}

async fn select<Reporter: self::Reporter + 'static>(
    shared: &Shared<'_>,
    mut workspace: workspace::Workspace,
    mut discovered: Vec<(PathBuf, Arc<manifest::Manifest>)>,
) -> Result<Vec<Project>> {
    let config = shared.context.config;
    let mut interpreters = Interpreters::new(config);
    let mut selected =
        shared.prepare_metadata::<Reporter>(&mut discovered, &mut interpreters).await?;
    workspace.update_manifests(&discovered);
    let mut projects = Vec::new();
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
        let requirements = Requirements::new(shared, &root, &manifest, &workspace)?;
        projects.push(Project { root, manifest, interpreter, environments, local, requirements });
    }
    Ok(projects)
}

struct Project {
    root: PathBuf,
    manifest: Arc<manifest::Manifest>,
    interpreter: Arc<super::Interpreter>,
    environments: Environments,
    local: Arc<[workspace::LocalProject]>,
    requirements: Requirements,
}

impl Project {
    async fn prepare<Reporter: self::Reporter + 'static>(
        self,
        shared: &Shared<'_>,
    ) -> Result<Prepared> {
        PythonPrepare::for_project(shared, &self.interpreter, &self.environments)
            .project::<Reporter>(self.root, self.manifest, self.local, self.requirements)
            .await
    }
}

pub(super) struct Requirements {
    pub(super) all: Vec<pep508_rs::Requirement>,
    pub(super) selected: Vec<pep508_rs::Requirement>,
    pub(super) rules: Arc<manifest::Manifest>,
}

impl Requirements {
    fn new(
        shared: &Shared<'_>,
        root: &Path,
        manifest: &Arc<manifest::Manifest>,
        workspace: &workspace::Workspace,
    ) -> Result<Self> {
        let requirements = manifest.selected_requirements(shared.context.config)?;
        Ok(Self {
            selected: workspace.requirements(
                root,
                manifest,
                requirements.selected(shared.asked.selection).to_vec(),
            )?,
            all: workspace.requirements(root, manifest, requirements.all)?,
            rules: workspace.resolution_manifest(root, manifest),
        })
    }
}
