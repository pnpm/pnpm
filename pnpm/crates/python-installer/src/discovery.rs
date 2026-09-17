//! The Python projects an install plan may act on, read once so that both
//! the workspace selection and the install itself see the same manifests.

use super::{manifest::Manifest, requirements, workspace::Workspace};
use miette::{IntoDiagnostic, Result, WrapErr};
use pep508_rs::PackageName;
use pnpm_workspace_projects_graph::{BaseProject, ProjectGraph, ProjectGraphNode};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Every Python project pnpm discovered, parsed.
///
/// A `--filter` selection narrows which of them an install acts on, and
/// the ones it leaves out are still read: a project installs the siblings
/// it declares a source for, so what those declare belongs to its own
/// resolution.
pub struct Discovery {
    pub(super) roots: Vec<(PathBuf, Arc<Manifest>)>,
    pub(super) workspace: Workspace,
}

/// A discovered project as the workspace filter sees it: the directory
/// that identifies it and the distribution it declares.
pub struct PythonProject<'a> {
    root: &'a Path,
    name: Option<&'a str>,
}

impl BaseProject for PythonProject<'_> {
    fn root_dir(&self) -> &Path {
        self.root
    }

    fn manifest_name(&self) -> Option<&str> {
        self.name
    }
}

/// Read every `pyproject.toml` and `requirements.txt` among `manifests`.
pub async fn discover(config: &pnpm_config::Config, manifests: &[PathBuf]) -> Result<Discovery> {
    let roots = read_project_manifests(manifests, config.workspace_dir.as_deref())
        .await?
        .into_iter()
        .map(|(root, manifest)| (root, Arc::new(manifest)))
        .collect::<Vec<_>>();
    let workspace = Workspace::new(&roots)?;
    Ok(Discovery { roots, workspace })
}

impl Discovery {
    /// Whether any discovered manifest declares a project. A manifest that
    /// only declares a workspace is read for what it says about the
    /// others, so it is not by itself a reason to start an interpreter.
    #[must_use]
    pub fn has_projects(&self) -> bool {
        self.roots.iter().any(|(_, manifest)| manifest.project.is_some())
    }

    /// The directory of every discovered project, in discovery order.
    pub fn project_roots(&self) -> impl Iterator<Item = &Path> {
        self.roots
            .iter()
            .filter(|(_, manifest)| manifest.project.is_some())
            .map(|(root, _)| root.as_path())
    }

    /// Read the manifests of `edited` again, for a command that changed
    /// them. Only those are read: the rest were parsed once and say the
    /// same thing they said then.
    pub(super) async fn reread(
        mut self,
        config: &pnpm_config::Config,
        edited: &BTreeSet<PathBuf>,
    ) -> Result<Self> {
        for root in edited {
            let path = root.join("pyproject.toml");
            let contents = tokio::fs::read_to_string(&path).await
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", path.display()))?;
            let allowed_root = config.workspace_dir.as_deref().unwrap_or(root);
            let manifest = Arc::new(read_manifest(&path, &contents, allowed_root).await?);
            match self.roots
                .iter_mut()
                .find(|(discovered, _)| discovered == root)
            {
                Some((_, discovered)) => *discovered = manifest,
                None => self.roots.push((root.clone(), manifest)),
            }
        }
        self.workspace = Workspace::new(&self.roots)?;
        Ok(self)
    }

    /// The discovered projects as the workspace dependency graph a
    /// `--filter` selector resolves against, keyed by project directory.
    #[must_use]
    pub fn graph(&self) -> ProjectGraph<PythonProject<'_>> {
        self.roots
            .iter()
            .filter(|(_, manifest)| manifest.project.is_some())
            .map(|(root, manifest)| {
                let package =
                    PythonProject { root, name: manifest.distribution().map(PackageName::as_ref) };
                let node = ProjectGraphNode {
                    package,
                    dependencies: self.workspace.declared_sources(root, manifest),
                };
                (root.clone(), node)
            })
            .collect()
    }
}

/// Every `pyproject.toml` among `manifests`, parsed. One without a
/// `[project]` table declares no package of its own, but may still say
/// which projects a workspace contains and where they come from.
async fn read_project_manifests(
    manifests: &[PathBuf],
    allowed_root: Option<&Path>,
) -> Result<Vec<(PathBuf, Manifest)>> {
    let mut roots = Vec::new();
    for path in manifests {
        let contents = if path
            .file_name()
            .is_some_and(|name| name == "requirements.txt")
        {
            String::new()
        } else {
            tokio::fs::read_to_string(path).await
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", path.display()))?
        };
        let allowed_root =
            allowed_root.unwrap_or_else(|| path.parent().expect("manifest has a parent"));
        let manifest = read_manifest(path, &contents, allowed_root).await?;
        if manifest.project.is_some()
            || manifest.tool.uv.workspace.is_some()
            || !manifest.tool.uv.overrides.is_empty()
            || !manifest.tool.uv.constraints.is_empty()
        {
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

async fn read_manifest(path: &Path, contents: &str, allowed_root: &Path) -> Result<Manifest> {
    let mut manifest = if path
        .file_name()
        .is_some_and(|name| name == "requirements.txt")
    {
        Manifest::parse("")?
    } else {
        Manifest::parse(contents)?
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
