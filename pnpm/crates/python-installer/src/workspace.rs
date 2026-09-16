use super::manifest::{Manifest, SourceDeclaration};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::Version;
use pep508_rs::{ExtraName, MarkerEnvironment, MarkerTree, PackageName, VerbatimUrl};
use pnpm_python_resolver::{LockedDirectory, Lockfile, Packages, WheelMetadata, parse_requirement};
use source::{Declared, path_target, reject_unresolvable, sole_source};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use wax::Program as _;

mod selection;
mod source;
use selection::Target;

/// A project in this repository that a resolution installs from its
/// source rather than from the index.
pub(super) struct LocalProject {
    pub(super) name: PackageName,
    pub(super) version: Version,
    pub(super) root: PathBuf,
    /// Installed so that edits to the project's source take effect without
    /// installing it again, which is what a project in the repository you
    /// are working in almost always wants.
    pub(super) editable: bool,
    pub(super) manifest: Arc<Manifest>,
    directory: LockedDirectory,
    metadata: WheelMetadata,
}

/// Offer only the projects a lockfile already names, so replaying it
/// cannot install one it does not describe. A project the lockfile has no
/// entry for is left out, and a resolution that needs it fails as stale
/// rather than reaching past what was locked.
pub(super) fn offer_locked(packages: &mut Packages, local: &[LocalProject], lock: &Lockfile) {
    let locked = lock.packages
        .iter()
        .map(|package| package.name.clone())
        .collect::<BTreeSet<_>>();
    for project in local {
        if locked.contains(&project.name) {
            packages.insert_directory(
                project.name.clone(),
                project.version.clone(),
                project.directory.clone(),
                project.metadata.clone(),
            );
        }
    }
}

/// Whether a lockfile still describes the workspace projects it pins.
///
/// What a locked directory package records is where its source is and how
/// it is installed, neither of which the solved graph can show. A
/// lockfile that pins another directory installs other code, so it is out
/// of date even when every version still agrees.
pub(super) fn describes(lock: &Lockfile, local: &[LocalProject]) -> Result<()> {
    for package in &lock.packages {
        let project = local
            .iter()
            .find(|project| project.name == package.name);
        match (&package.directory, project) {
            (Some(directory), Some(project)) => {
                if project.version != package.version || project.directory != *directory {
                    bail!("the Python workspace project `{}` changed", package.name);
                }
            }
            (Some(_), None) => {
                bail!("the Python project `{}` is no longer a workspace source", package.name)
            }
            (None, Some(project)) => bail!(
                "the Python distribution `{}` is now the workspace project at {}",
                package.name,
                project.root.display(),
            ),
            (None, None) => {}
        }
    }
    Ok(())
}

/// Offer these projects to a resolution as the only version of the
/// distribution each one declares.
///
/// A resolution is seeded more than once: a replayed lockfile names them
/// too, and what their manifests say now is what the replay is checked
/// against.
pub(super) fn offer(packages: &mut Packages, local: &[LocalProject]) {
    for project in local {
        packages.insert_directory(
            project.name.clone(),
            project.version.clone(),
            project.directory.clone(),
            project.metadata.clone(),
        );
    }
}

/// The Python projects pnpm discovered, indexed by the distribution each
/// one declares.
///
/// A requirement naming one of them is satisfied by that project rather
/// than by the index, but only where the requiring project says so in
/// `[tool.uv.sources]`. Without that declaration the requirement is
/// refused: resolving it from the index would install different code
/// under the same name.
pub(super) struct Workspace {
    declared: BTreeMap<PathBuf, BTreeSet<PackageName>>,
    /// Where each distribution is declared. Two projects may declare one
    /// name without that being wrong: they are only in conflict if
    /// something depends on the name.
    roots: BTreeMap<PackageName, Vec<PathBuf>>,
    manifests: BTreeMap<PathBuf, Arc<Manifest>>,
    inherited: BTreeMap<PathBuf, (PathBuf, Arc<Manifest>)>,
    selection: Option<Selection>,
}

impl Workspace {
    /// Which distributions each discovered project may take from the
    /// repository, which is what a build requirement of that project is
    /// read against too.
    pub(super) fn scopes(&self) -> &BTreeMap<PathBuf, BTreeSet<PackageName>> {
        &self.declared
    }

    pub(super) fn new(projects: &[(PathBuf, Arc<Manifest>)]) -> Result<Self> {
        let mut roots = BTreeMap::<PackageName, Vec<PathBuf>>::new();
        let mut declared = BTreeMap::new();
        for (root, manifest) in projects {
            if let Some(name) = manifest.distribution() {
                roots
                    .entry(name.clone())
                    .or_default()
                    .push(root.clone());
            }
            declared.insert(root.clone(), BTreeSet::new());
        }
        let manifests = projects
            .iter()
            .map(|(root, manifest)| (root.clone(), Arc::clone(manifest)))
            .collect();
        let workspace =
            Self { declared, roots, manifests, inherited: BTreeMap::new(), selection: None };
        workspace.with_scopes(projects)
    }

    pub(super) fn update_manifests(&mut self, projects: &[(PathBuf, Arc<Manifest>)]) {
        self.manifests = projects
            .iter()
            .map(|(root, manifest)| (root.clone(), Arc::clone(manifest)))
            .collect();
    }

    pub(super) fn for_resolution(
        &mut self,
        config: &'static pnpm_config::Config,
        environments: &super::targets::Environments,
    ) {
        self.selection = Some(Selection {
            config,
            environments: environments.list
                .iter()
                .map(|environment| environment.target.environment.clone())
                .collect(),
        });
    }

    /// Which distributions each project may take from the repository.
    ///
    /// A `[tool.uv.workspace]` table says which projects under it the
    /// workspace contains, and a project inside one may only take those.
    /// Without such a table there is no declared workspace, and every
    /// project pnpm discovered is one pnpm may link.
    fn with_scopes(mut self, projects: &[(PathBuf, Arc<Manifest>)]) -> Result<Self> {
        let every = self.roots
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        for (root, _) in projects {
            let members = match Self::declaring_root(root, projects) {
                Some((declared_in, declaration, manifest)) => {
                    if declared_in != *root {
                        self.inherited.insert(
                            root.clone(),
                            (declared_in.clone(), Arc::clone(manifest)),
                        );
                    }
                    members_of(&declared_in, declaration, projects)
                }
                None => every.clone(),
            };
            self.declared.insert(root.clone(), members);
        }
        Ok(self)
    }

    /// Where the project declaring `name` is, for a project at `root` that
    /// may depend on it.
    fn member(&self, name: &PackageName, root: &Path) -> Result<Option<&Path>> {
        if !self.declared
            .get(root)
            .is_some_and(|members| members.contains(name))
        {
            return Ok(None);
        }
        match self.roots.get(name).map(Vec::as_slice) {
            None | Some([]) => Ok(None),
            Some([only]) => Ok(Some(only)),
            Some([first, second, ..]) => bail!(
                "two Python projects in this workspace declare `{name}`: {} and {}",
                first.display(),
                second.display(),
            ),
        }
    }

    /// The nearest manifest at or above `root` that declares a workspace.
    fn declaring_root<'a>(
        root: &Path,
        projects: &'a [(PathBuf, Arc<Manifest>)],
    ) -> Option<(PathBuf, &'a crate::manifest::UvWorkspace, &'a Arc<Manifest>)> {
        for ancestor in root.ancestors() {
            let found = projects
                .iter()
                .find(|(candidate, _)| candidate == ancestor);
            if let Some((candidate, manifest)) = found
                && let Some(declaration) = manifest.tool.uv.workspace.as_ref()
            {
                return Some((candidate.clone(), declaration, manifest));
            }
        }
        None
    }

    /// Every workspace project the project at `root` reaches through its
    /// requirements, and theirs.
    ///
    /// Only a project's own requirements consult its `[tool.uv.sources]`,
    /// so what an index package requires is resolved from the index, as it
    /// is for the tool that wrote the sources table. `lock_root` is the
    /// directory the lockfile recording these projects sits in, which
    /// their recorded paths are relative to.
    pub(super) fn local_projects(
        &self,
        root: &Path,
        manifest: &Manifest,
        lock_root: &Path,
    ) -> Result<Vec<LocalProject>> {
        let mut local = Vec::new();
        let mut seen = BTreeMap::<PackageName, Target>::new();
        let mut frontier = self.targets(root, manifest, None)?;
        while let Some((name, target)) = frontier.pop() {
            if let Some(chosen) = seen.get_mut(&name) {
                if chosen.merge(&target, &name, root)? {
                    let manifest = self.load(&target.root)?;
                    frontier.extend(self.targets(&target.root, &manifest, Some(&chosen.extras))?);
                }
                continue;
            }
            seen.insert(
                name.clone(),
                Target {
                    root: target.root.clone(),
                    editable: target.editable,
                    extras: target.extras.clone(),
                },
            );
            let manifest = self.load(&target.root)?;
            local.push(read_project(&name, &target, &manifest, lock_root)?);
            frontier.extend(self.targets(&target.root, &manifest, Some(&target.extras))?);
        }
        Ok(local)
    }

    fn load(&self, root: &Path) -> Result<Arc<Manifest>> {
        match self.manifests.get(root) {
            Some(manifest) => Ok(Arc::clone(manifest)),
            None => Ok(Arc::new(load(root)?)),
        }
    }

    /// Which of a project's requirements name a project on disk, and where
    /// each one lives.
    fn targets(
        &self,
        root: &Path,
        manifest: &Manifest,
        extras: Option<&BTreeSet<ExtraName>>,
    ) -> Result<Vec<(PackageName, Target)>> {
        let mut targets = Vec::new();
        for (name, extras) in self.selected_distributions(manifest, extras)? {
            match self.source(root, manifest, &name) {
                Some(Declared { declaration, by }) => {
                    let mut target = self.target(&name, declaration, root, by)?;
                    target.extras = extras;
                    targets.push((name, target));
                }
                None => self.refuse_shadowed_member(&name, root)?,
            }
        }
        Ok(targets)
    }

    /// The source a project resolves a distribution from: its own
    /// declaration, else the one its workspace root declares. A member
    /// inherits the root's table, so a workspace can say once where each
    /// of its projects comes from.
    fn source<'a>(
        &'a self,
        root: &'a Path,
        manifest: &'a Manifest,
        name: &PackageName,
    ) -> Option<Declared<'a>> {
        if let Some(declaration) = manifest.tool.uv.sources.get(name) {
            return Some(Declared { declaration, by: root });
        }
        let (declaring_root, inherited) = self.inherited.get(root)?;
        Some(Declared { declaration: inherited.tool.uv.sources.get(name)?, by: declaring_root })
    }

    /// Refuse to resolve a workspace project's distribution from the
    /// index. Silently installing the index's package under that name
    /// would install code the repository did not write.
    fn refuse_shadowed_member(&self, name: &PackageName, root: &Path) -> Result<()> {
        let Some(member) = self.member(name, root)? else { return Ok(()) };
        if member == root {
            return Ok(());
        }
        bail!(
            "the Python requirement `{name}` names a project in this workspace ({}), but {} does \
             not say where it comes from. Declare `{name} = {{ workspace = true }}` under \
             [tool.uv.sources] to depend on that project. pnpm does not resolve a name the \
             workspace declares from the index, because that installs different code under it.",
            member.display(),
            root.join("pyproject.toml").display(),
        )
    }

    fn target(
        &self,
        name: &PackageName,
        declaration: &SourceDeclaration,
        root: &Path,
        declared_by: &Path,
    ) -> Result<Target> {
        let manifest_path = declared_by.join("pyproject.toml");
        let source = sole_source(declaration, name, &manifest_path)?;
        reject_unresolvable(source, name, &manifest_path)?;
        if let Some(path) = &source.path {
            return Ok(Target {
                root: path_target(declared_by, path, name, &manifest_path)?,
                editable: source.editable.unwrap_or(false),
                extras: BTreeSet::new(),
            });
        }
        let Some(member) = self.member(name, root)? else {
            let manifest = manifest_path.display();
            bail!(
                "{manifest} declares `{name}` as a workspace Python source, but no project in \
                 its workspace declares `{name}`",
            );
        };
        Ok(Target {
            root: member.to_path_buf(),
            editable: source.editable.unwrap_or(true),
            extras: BTreeSet::new(),
        })
    }
}

struct Selection {
    config: &'static pnpm_config::Config,
    environments: Vec<MarkerEnvironment>,
}

/// The distributions a workspace contains: the root's own, and every
/// discovered project under a `members` pattern that no `exclude` pattern
/// takes back.
fn members_of(
    root: &Path,
    declaration: &crate::manifest::UvWorkspace,
    projects: &[(PathBuf, Arc<Manifest>)],
) -> BTreeSet<PackageName> {
    let included = compile(&declaration.members);
    let excluded = compile(&declaration.exclude);
    let mut members = BTreeSet::new();
    for (candidate, manifest) in projects {
        let Some(name) = manifest.distribution() else { continue };
        let Ok(relative) = candidate.strip_prefix(root) else { continue };
        let relative = if relative.as_os_str().is_empty() { Path::new(".") } else { relative };
        if candidate == root
            || (included
                .iter()
                .any(|glob| glob.is_match(relative))
                && !excluded
                    .iter()
                    .any(|glob| glob.is_match(relative)))
        {
            members.insert(name.clone());
        }
    }
    members
}

/// A pattern that cannot be read matches nothing, the way a member
/// directory that is not there contains no project.
fn compile(patterns: &[String]) -> Vec<wax::Glob<'static>> {
    patterns
        .iter()
        .filter_map(|pattern| wax::Glob::new(pattern).ok().map(wax::Glob::into_owned))
        .collect()
}

fn load(root: &Path) -> Result<Manifest> {
    let path = root.join("pyproject.toml");
    let contents = std::fs::read_to_string(&path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read the Python project at {}", path.display()))?;
    Manifest::parse(&contents)
}

fn read_project(
    name: &PackageName,
    target: &Target,
    manifest: &Arc<Manifest>,
    lock_root: &Path,
) -> Result<LocalProject> {
    let path = target.root.join("pyproject.toml");
    let Some(project) = &manifest.project else {
        bail!("the Python project at {} declares no [project] table", path.display());
    };
    if manifest.distribution() != Some(name) {
        bail!("the Python project at {} does not declare `{name}`", path.display());
    }
    if manifest.tool.uv.package == Some(false) {
        bail!(
            "the Python project at {} sets `tool.uv.package = false`, so it builds no package for \
             `{name}` to install",
            path.display(),
        );
    }
    let Some(version) = project.version.clone() else {
        bail!(
            "pnpm needs a static `version` for the Python project at {}: a workspace project is \
             read from its manifest, not built to find out what it declares",
            path.display(),
        );
    };
    Ok(LocalProject {
        directory: LockedDirectory {
            path: relative(lock_root, &target.root),
            editable: target.editable,
        },
        metadata: WheelMetadata {
            name: name.to_string(),
            version: version.to_string(),
            requires_dist: manifest.distribution_requirements()?,
            requires_python: project.requires_python.clone(),
            provides_extra: manifest.extras(),
        },
        name: name.clone(),
        version,
        root: target.root.clone(),
        editable: target.editable,
        manifest: Arc::clone(manifest),
    })
}

/// A requirement as a wheel's `METADATA` would state it: a requirement an
/// extra brings in carries the marker that selects that extra.
pub(super) fn requirement_for_extra(requirement: &str, extra: &str) -> Result<String> {
    let mut parsed = parse_requirement(requirement)?;
    let mut marker = MarkerTree::parse_str::<VerbatimUrl>(&format!("extra == {extra:?}"))
        .into_diagnostic()
        .wrap_err_with(|| format!("build the marker selecting the Python extra {extra}"))?;
    marker.and(parsed.marker);
    parsed.marker = marker;
    Ok(parsed.to_string())
}

/// How a lockfile in `from` spells the project at `to`, in the relative
/// POSIX form PEP 751 records. Relative is what makes the lockfile mean
/// the same thing in another checkout, so only a project on another
/// filesystem root keeps an absolute path, which is what
/// [`pnpm_fs::relative_path`] falls back to.
pub(super) fn relative(from: &Path, to: &Path) -> String {
    let relative = pnpm_fs::relative_path(from, to)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    pnpm_fs::lexical_normalize_posix(&relative)
}
