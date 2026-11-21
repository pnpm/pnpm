//! The Python projects an install plan may act on, read once so that both
//! the workspace selection and the install itself see the same manifests.

use super::{
    environment::{
        EnvironmentLink,
        EnvironmentStore,
    },
    manifest::{
        Manifest,
        RequirementScope,
    },
    requirements,
    workspace::{
        Workspace,
        members::DeclaredWorkspaces,
    },
};
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
};
use pep508_rs::PackageName;
use pnpm_workspace_projects_graph::{
    BaseProject,
    ProjectGraph,
    ProjectGraphNode,
};
use std::{
    cmp::Ordering,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

const DEFAULT_IGNORED_DIRECTORY_BASENAMES: &[&str] = &[
    "demo",
    "demos",
    "docs",
    "example",
    "examples",
    "fixture",
    "fixtures",
    "sample",
    "samples",
    "template",
    "templates",
    "test",
    "test-fixtures",
    "test_fixtures",
    "tests",
];

/// Every Python project pnpm discovered, parsed.
///
/// A `--filter` selection narrows which of them an install acts on, and
/// the ones it leaves out are still read: a project installs the siblings
/// it declares a source for, so what those declare belongs to its own
/// resolution.
pub struct Discovery {
    pub(super) roots: Vec<(PathBuf, Arc<Manifest>)>,
    pub(super) workspace: Workspace,
    /// The requirements-only directories whose `.venv` is unmanaged.
    /// Such a `requirements.txt` often lists tooling its user installs by
    /// other means, so the directory is not a project.
    pub(super) unmanaged: Vec<PathBuf>,
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

/// Read the Python projects admitted by declared workspaces and the default
/// discovery conventions among `manifests`.
pub async fn discover(
    config: &pnpm_config::Config,
    workspace_root: &Path,
    manifests: &[PathBuf],
) -> Result<Discovery> {
    let reading = Reading {
        workspace_root,
        allowed_root: config.workspace_dir.as_deref(),
        environments: EnvironmentStore::new(config),
    };
    let (roots, unmanaged) = read_project_manifests(manifests, reading).await?;
    let roots = roots
        .into_iter()
        .map(|(root, manifest)| (root, Arc::new(manifest)))
        .collect::<Vec<_>>();
    let workspace = Workspace::new(&roots)?;
    Ok(Discovery { roots, workspace, unmanaged })
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

    /// The project directories a command targeting `dir` acts on: all
    /// members of the shared environment rooted at `dir` when `dir`
    /// declares one, `dir` itself when it declares a `[project]`, or none
    /// for an unshared virtual workspace root.
    #[must_use]
    pub fn target_projects(&self, dir: &Path) -> Vec<PathBuf> {
        self.workspace.target_projects(dir)
    }

    /// Read the manifests of `edited` again, for a command that changed
    /// them. Only those are read: the rest were parsed once and say the
    /// same thing they said then.
    pub(super) async fn reread(
        mut self,
        config: &pnpm_config::Config,
        edited: &BTreeSet<PathBuf>,
    ) -> Result<Self> {
        let environments = EnvironmentStore::new(config);
        for root in edited {
            let path = root.join("pyproject.toml");
            let allowed_root = config.workspace_dir.as_deref().unwrap_or(root);
            let own_environment = (self.workspace.lock_root(root) == root).then_some(&environments);
            let adjacent = Adjacent { allowed_root, own_environment };
            let manifest = Arc::new(read_member_manifest(&path, adjacent).await?);
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
        self.graph_of(RequirementScope::All)
    }

    /// [`Self::graph`] for a `--filter-prod` selector: a dependency group is
    /// a development input, so a source only a group requires is no edge.
    #[must_use]
    pub fn production_graph(&self) -> ProjectGraph<PythonProject<'_>> {
        self.graph_of(RequirementScope::Production)
    }

    fn graph_of(&self, scope: RequirementScope) -> ProjectGraph<PythonProject<'_>> {
        self.roots
            .iter()
            .filter(|(_, manifest)| manifest.project.is_some())
            .map(|(root, manifest)| {
                let package =
                    PythonProject { root, name: manifest.distribution().map(PackageName::as_ref) };
                let node = ProjectGraphNode {
                    package,
                    dependencies: self.workspace.declared_sources(root, manifest, scope),
                };
                (root.clone(), node)
            })
            .collect()
    }
}

struct Reading<'a> {
    workspace_root: &'a Path,
    allowed_root: Option<&'a Path>,
    environments: EnvironmentStore,
}

/// The Python manifests admitted by the nearest `[tool.uv.workspace]`, and
/// the directories [`Discovery::unmanaged`] names.
/// Outside a declared workspace, conventional sample and fixture directories
/// are not projects unless a declaration explicitly includes them.
async fn read_project_manifests(
    manifests: &[PathBuf],
    reading: Reading<'_>,
) -> Result<(Vec<(PathBuf, Manifest)>, Vec<PathBuf>)> {
    let (pyprojects, declared) = read_pyprojects(manifests, reading.workspace_root).await?;
    let mut candidates = ProjectManifests { pyprojects, declared, reading };
    let mut roots = Vec::new();
    let mut unmanaged = Vec::new();
    for path in manifests {
        let root = path.parent().expect("manifest has a parent");
        let Some(manifest) = candidates.read(path).await? else { continue };
        if manifest.has_unmanaged_requirements {
            unmanaged.push(root.to_path_buf());
        }
        if declares_python_project(&manifest) {
            roots.push((root.to_path_buf(), manifest));
        }
    }
    Ok((roots, unmanaged))
}

struct ProjectManifests<'a> {
    pyprojects: BTreeMap<PathBuf, Manifest>,
    declared: DeclaredWorkspaces,
    reading: Reading<'a>,
}

impl ProjectManifests<'_> {
    async fn read(&mut self, path: &Path) -> Result<Option<Manifest>> {
        let root = path.parent().expect("manifest has a parent");
        let adjacent = Adjacent {
            allowed_root: self.reading.allowed_root.unwrap_or(root),
            own_environment: (!self.declared.shares_environment(root)).then_some(
                &self.reading.environments,
            ),
        };
        if is_pyproject(path) {
            let Some(manifest) = self.pyprojects.remove(path) else { return Ok(None) };
            return read_adjacent_requirements(path, manifest, adjacent).await.map(Some);
        }
        if !is_discovered(root, self.reading.workspace_root, &self.declared) {
            return Ok(None);
        }
        read_adjacent_requirements(path, Manifest::parse("")?, adjacent).await.map(Some)
    }
}

fn declares_python_project(manifest: &Manifest) -> bool {
    manifest.project.is_some()
        || manifest.tool.uv.workspace.is_some()
        || !manifest.tool.uv.overrides.is_empty()
        || !manifest.tool.uv.constraints.is_empty()
}

async fn read_pyprojects(
    manifests: &[PathBuf],
    workspace_root: &Path,
) -> Result<(BTreeMap<PathBuf, Manifest>, DeclaredWorkspaces)> {
    let mut pyprojects = BTreeMap::new();
    let mut paths = manifests
        .iter()
        .filter(|path| is_pyproject(path))
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| compare_manifest_paths(left, right));
    let mut declared = DeclaredWorkspaces::default();
    for path in paths {
        let root = path.parent().expect("manifest has a parent");
        let declared_membership = declared.contains(root);
        let workspace_declaration_only = !declared_membership.unwrap_or(true)
            || declared_membership.is_none() && is_ignored_by_default(root, workspace_root);
        let Some(manifest) = read_pyproject(path, workspace_declaration_only).await? else {
            continue;
        };
        declared.add(root, &manifest);
        pyprojects.insert(path.clone(), manifest);
    }
    Ok((pyprojects, declared))
}

async fn read_pyproject(path: &Path, workspace_declaration_only: bool) -> Result<Option<Manifest>> {
    let contents = match tokio::fs::read(path).await {
        Ok(contents) => contents,
        Err(_) if workspace_declaration_only => return Ok(None),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", path.display()));
        }
    };
    let contents = match std::str::from_utf8(&contents) {
        Ok(contents) => contents,
        Err(_) if workspace_declaration_only => return Ok(None),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("decode {}", path.display()));
        }
    };
    let manifest = match Manifest::parse(contents) {
        Ok(manifest) => manifest,
        Err(_) if workspace_declaration_only => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok((!workspace_declaration_only || manifest.tool.uv.workspace.is_some()).then_some(manifest))
}

fn compare_manifest_paths(left: &Path, right: &Path) -> Ordering {
    let depth = |path: &Path| {
        path.parent()
            .expect("manifest has a parent")
            .components()
            .count()
    };
    depth(left)
        .cmp(&depth(right))
        .then_with(|| left.cmp(right))
}

fn is_pyproject(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == "pyproject.toml")
}

fn is_discovered(project: &Path, workspace_root: &Path, declared: &DeclaredWorkspaces) -> bool {
    declared
        .contains(project)
        .unwrap_or_else(|| !is_ignored_by_default(project, workspace_root))
}

fn is_ignored_by_default(project: &Path, workspace_root: &Path) -> bool {
    project
        .strip_prefix(workspace_root)
        .is_ok_and(|relative| {
            relative
                .components()
                .any(|component| {
                    component
                        .as_os_str()
                        .to_str()
                        .is_some_and(|basename| {
                            DEFAULT_IGNORED_DIRECTORY_BASENAMES.contains(&basename)
                        })
                })
        })
}

async fn read_member_manifest(path: &Path, adjacent: Adjacent<'_>) -> Result<Manifest> {
    let contents = match tokio::fs::read_to_string(path).await {
        Ok(contents) => contents,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && tokio::fs::try_exists(path.with_file_name("requirements.txt"))
                    .await
                    .into_diagnostic()? =>
        {
            String::new()
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", path.display()));
        }
    };
    let manifest = Manifest::parse(&contents)?;
    read_adjacent_requirements(path, manifest, adjacent).await
}

/// What reading a `requirements.txt` beside a manifest needs.
#[derive(Clone, Copy)]
struct Adjacent<'a> {
    allowed_root: &'a Path,
    /// The store that judges the directory's `.venv`, or `None` for a
    /// member of a shared environment, which installs into no `.venv` of
    /// its own.
    own_environment: Option<&'a EnvironmentStore>,
}

/// Read the `requirements.txt` beside a manifest that declares no
/// `[project]` as its dependencies. A manifest that declares a workspace
/// is excluded: like uv's virtual workspace root, it is not a member of
/// its own workspace, and a `requirements.txt` there usually lists tooling
/// installed by other means. So is a directory [`Discovery::unmanaged`]
/// names, whose file is left unread.
async fn read_adjacent_requirements(
    path: &Path,
    mut manifest: Manifest,
    adjacent: Adjacent<'_>,
) -> Result<Manifest> {
    if manifest.project.is_some() || manifest.tool.uv.workspace.is_some() {
        return Ok(manifest);
    }
    let requirements_path = path.with_file_name("requirements.txt");
    if !tokio::fs::try_exists(&requirements_path).await.into_diagnostic()? {
        return Ok(manifest);
    }
    let allowed_root = adjacent.allowed_root.to_path_buf();
    let own_environment = adjacent.own_environment.cloned();
    let requirements = tokio::task::spawn_blocking(move || {
        read_managed_requirements(own_environment.as_ref(), &requirements_path, &allowed_root)
    })
    .await
    .into_diagnostic()
    .wrap_err("join Python requirements parsing")??;
    match requirements {
        Some(requirements) => manifest.set_requirements_file(requirements),
        None => manifest.has_unmanaged_requirements = true,
    }
    Ok(manifest)
}

/// The requirements `path` lists, or `None` when the directory installs
/// into a `.venv` of its own that is unmanaged.
fn read_managed_requirements(
    own_environment: Option<&EnvironmentStore>,
    path: &Path,
    allowed_root: &Path,
) -> Result<Option<Vec<String>>> {
    let root = path.parent().expect("requirements file has a parent");
    if let Some(environments) = own_environment
        && matches!(environments.link(root)?, EnvironmentLink::Unmanaged)
    {
        return Ok(None);
    }
    requirements::read(path, allowed_root).map(Some)
}

#[cfg(test)]
mod tests;
