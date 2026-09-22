//! Which projects a declared workspace contains, and which of them
//! install into one environment: a project on its own by default, and the
//! members of a workspace whose root asks for a shared one together.
//!
//! Sharing is what `[tool.uv.workspace]` already draws the boundary of.
//! It is opt-in because uv's model, which always shares, forces a
//! repository whose projects genuinely conflict to split into separate
//! workspaces; per-project resolution needs no such split.

use super::{
    Manifest,
    Workspace,
};
use crate::manifest::UvWorkspace;
use miette::{
    Result,
    bail,
};
use pep508_rs::PackageName;
use std::{
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
use wax::Program as _;

/// The projects one lockfile and one environment answer for, and the
/// directory both live in.
pub(crate) struct Membership {
    pub(crate) root: PathBuf,
    pub(crate) members: Vec<PathBuf>,
    /// Whether the members were asked to share. A project of its own is
    /// the one member of an unshared membership.
    pub(crate) shared: bool,
}

impl Workspace {
    /// Read which projects share an environment. A workspace root asks
    /// for that with `shared-environment` under `[tool.pnpm.python]`, and
    /// only a manifest that declares a workspace has members to share it.
    /// Two members declaring one distribution are refused: each would be
    /// installed as itself, and an environment holds one of a name.
    pub(super) fn with_shared(mut self, projects: &[(PathBuf, Arc<Manifest>)]) -> Result<Self> {
        for (root, manifest) in projects {
            if !manifest.shares_environment() {
                continue;
            }
            let Some(declaration) = &manifest.tool.uv.workspace else {
                bail!(
                    "{} asks for a shared Python environment, but declares no [tool.uv.workspace] \
                     whose members would share it",
                    root.join("pyproject.toml").display(),
                );
            };
            self.share_members(root, declaration, projects)?;
        }
        Ok(self)
    }

    /// Record every project the workspace at `root` contains as sharing
    /// its environment.
    fn share_members(
        &mut self,
        root: &Path,
        declaration: &UvWorkspace,
        projects: &[(PathBuf, Arc<Manifest>)],
    ) -> Result<()> {
        let mut declared = BTreeMap::<&PackageName, &Path>::new();
        for (member, manifest) in member_projects(root, declaration, projects) {
            if manifest.project.is_none() || self.declared_by(member) != Some(root) {
                continue;
            }
            if let Some(name) = manifest.distribution()
                && let Some(other) = declared.insert(name, member)
            {
                bail!(
                    "the Python projects at {} and {} both declare `{name}`, and one environment \
                     cannot install both; they share the environment at {}",
                    other.display(),
                    member.display(),
                    root.display(),
                );
            }
            self.shared.insert(member.clone(), root.to_path_buf());
        }
        Ok(())
    }

    /// The workspace the project at `member` belongs to: the nearest one
    /// declared at or above it, which reading the scopes already found.
    fn declared_by<'a>(&'a self, member: &'a Path) -> Option<&'a Path> {
        if let Some((declared_in, _)) = self.inherited.get(member) {
            return Some(declared_in);
        }
        self.manifests
            .get(member)
            .filter(|manifest| manifest.tool.uv.workspace.is_some())
            .map(|_| member)
    }

    /// The directory whose lockfile and environment the project at `root`
    /// installs into: its workspace root when that workspace shares one,
    /// and its own otherwise.
    pub(crate) fn lock_root<'a>(&'a self, root: &'a Path) -> &'a Path {
        self.shared.get(root).map_or(root, PathBuf::as_path)
    }

    /// The selected projects, grouped by what they install into.
    ///
    /// A shared environment is one thing, so selecting any member of it
    /// asks for the whole membership: an environment holding only the
    /// selected members would leave the others unable to run.
    pub(crate) fn memberships(&self, selected: &BTreeSet<PathBuf>) -> Vec<Membership> {
        let mut grouped = BTreeMap::<&Path, Membership>::new();
        for root in selected {
            let lock_root = self.lock_root(root);
            grouped
                .entry(lock_root)
                .or_insert_with(|| self.membership(lock_root));
        }
        grouped.into_values().collect()
    }

    fn membership(&self, lock_root: &Path) -> Membership {
        let members = self.shared
            .iter()
            .filter(|(_, root)| root.as_path() == lock_root)
            .map(|(member, _)| member.clone())
            .collect::<Vec<_>>();
        if members.is_empty() {
            return Membership {
                root: lock_root.to_path_buf(),
                members: vec![lock_root.to_path_buf()],
                shared: false,
            };
        }
        Membership { root: lock_root.to_path_buf(), members, shared: true }
    }
}

/// The distributions a workspace contains, which is what a member may
/// take from the repository.
pub(super) fn members_of(
    root: &Path,
    declaration: &UvWorkspace,
    projects: &[(PathBuf, Arc<Manifest>)],
) -> BTreeSet<PackageName> {
    member_projects(root, declaration, projects)
        .filter_map(|(_, manifest)| manifest.distribution().cloned())
        .collect()
}

/// The projects a workspace contains: the root's own, and every
/// discovered project under a `members` pattern that no `exclude` pattern
/// takes back.
fn member_projects<'a>(
    root: &'a Path,
    declaration: &UvWorkspace,
    projects: &'a [(PathBuf, Arc<Manifest>)],
) -> impl Iterator<Item = (&'a PathBuf, &'a Arc<Manifest>)> {
    let patterns = Patterns::of(declaration);
    projects
        .iter()
        .filter(move |(candidate, _)| patterns.contain(root, candidate))
        .map(|(candidate, manifest)| (candidate, manifest))
}

/// The workspace declarations already encountered while walking projects
/// from shallow paths to deep ones.
#[derive(Default)]
pub(crate) struct DeclaredWorkspaces {
    declarations: BTreeMap<PathBuf, Patterns>,
}

impl DeclaredWorkspaces {
    pub(crate) fn add(&mut self, root: &Path, manifest: &Manifest) {
        let Some(declaration) = manifest.tool.uv.workspace.as_ref() else { return };
        self.declarations.insert(root.to_path_buf(), Patterns::of(declaration));
    }

    /// Whether the nearest workspace declaration contains `candidate`.
    pub(crate) fn contains(&self, candidate: &Path) -> Option<bool> {
        candidate
            .ancestors()
            .find_map(|root| {
                self.declarations
                    .get(root)
                    .map(|patterns| patterns.contain(root, candidate))
            })
    }
}

struct Patterns {
    included: Vec<wax::Glob<'static>>,
    excluded: Vec<wax::Glob<'static>>,
}

impl Patterns {
    fn of(declaration: &UvWorkspace) -> Self {
        Self { included: compile(&declaration.members), excluded: compile(&declaration.exclude) }
    }

    /// Whether the workspace at `root` contains the project at `candidate`.
    fn contain(&self, root: &Path, candidate: &Path) -> bool {
        let Ok(relative) = candidate.strip_prefix(root) else { return false };
        let relative = if relative.as_os_str().is_empty() { Path::new(".") } else { relative };
        candidate == root
            || (self.included
                .iter()
                .any(|glob| glob.is_match(relative))
                && !self.excluded
                    .iter()
                    .any(|glob| glob.is_match(relative)))
    }
}

/// The directory whose environment a command run in `dir` uses: the root
/// of the workspace the project containing `dir` shares an environment
/// with, else that project's own directory. Read from the manifests
/// between `dir` and `workspace`, the way an install reads them: the
/// project is the nearest directory with a manifest, and the workspace it
/// belongs to is the nearest one declared at or above it. A command reads
/// this without an install's discovery, so a manifest that does not
/// parse counts for nothing here and is reported by the next install.
pub(crate) fn environment_root_of(workspace: Option<&Path>, dir: &Path) -> PathBuf {
    let Some(stop) = workspace else { return dir.to_path_buf() };
    // The workspace may be configured by a path that reaches it through
    // a link, while the command's directory is canonical.
    let stop = canonical(stop);
    let dir = canonical(dir);
    let project = dir
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(&stop))
        .find(|ancestor| ancestor.join("pyproject.toml").is_file())
        .unwrap_or(&dir);
    let declared = project
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(&stop))
        .find_map(|ancestor| Some((ancestor, workspace_declaration(ancestor)?)));
    match declared {
        Some((root, (true, patterns))) if patterns.contain(root, project) => root.to_path_buf(),
        _ => project.to_path_buf(),
    }
}

/// Whether a `[tool.uv.workspace]` is declared at or above `project`, up
/// to `workspace`. A project in one may take its siblings from the
/// repository and may share their environment, so an add there reads
/// the workspace around it; a project outside any reads itself alone.
#[must_use]
pub fn in_declared_workspace(workspace: &Path, project: &Path) -> bool {
    let stop = canonical(workspace);
    canonical(project)
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(&stop))
        .any(|ancestor| workspace_declaration(ancestor).is_some())
}

/// The path with links resolved, or as given where that fails: a path
/// that cannot be resolved is still the one the caller means.
fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether the manifest at `root` shares its environment, and which
/// projects it contains, for one that declares a workspace.
fn workspace_declaration(root: &Path) -> Option<(bool, Patterns)> {
    let manifest = std::fs::read_to_string(root.join("pyproject.toml"))
        .ok()
        .and_then(|contents| Manifest::parse(&contents).ok())?;
    let declaration = manifest.tool.uv.workspace.as_ref()?;
    Some((manifest.shares_environment(), Patterns::of(declaration)))
}

/// A pattern that cannot be read matches nothing, the way a member
/// directory that is not there contains no project.
fn compile(patterns: &[String]) -> Vec<wax::Glob<'static>> {
    patterns
        .iter()
        .filter_map(|pattern| wax::Glob::new(pattern).ok().map(wax::Glob::into_owned))
        .collect()
}
