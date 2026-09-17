//! What one install prepares: a unit per lockfile and environment, which
//! is a project on its own or the members of a workspace sharing one.

pub(super) use disagreement::disagreement;

mod disagreement;

use super::{
    Environments, Interpreter, Interpreters, Prepared,
    environment::{PythonPrepare, Shared},
    interpreter, manifest,
    workspace::{self, members::Membership},
};
use futures_util::{StreamExt, stream};
use miette::Result;
use pnpm_reporter::Reporter;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) async fn prepare<Reporter: self::Reporter + 'static>(
    shared: &Shared<'_>,
    workspace: workspace::Workspace,
    discovered: Vec<(PathBuf, Arc<manifest::Manifest>)>,
    selected: &BTreeSet<PathBuf>,
) -> Result<Vec<Prepared>> {
    let config = shared.context.config;
    let units = select::<Reporter>(shared, workspace, discovered, selected).await?;
    let results = stream::iter(units.into_iter().enumerate())
        .map(|(position, unit)| async move { (position, unit.prepare::<Reporter>(shared).await) })
        .buffer_unordered(config.workspace_concurrency.max(1) as usize)
        .collect::<BTreeMap<_, _>>()
        .await;
    results.into_values().collect()
}

/// The units the install prepares: those the workspace selection asked
/// for. The other projects are read for what a selected one's declared
/// sources need, and nothing else.
async fn select<Reporter: self::Reporter + 'static>(
    shared: &Shared<'_>,
    mut workspace: workspace::Workspace,
    mut discovered: Vec<(PathBuf, Arc<manifest::Manifest>)>,
    selected: &BTreeSet<PathBuf>,
) -> Result<Vec<Unit>> {
    let config = shared.context.config;
    let mut interpreters = Interpreters::new(config, &shared.context.http_client);
    let memberships = workspace.memberships(selected);
    let needed = memberships
        .iter()
        .flat_map(|membership| membership.members.iter().cloned())
        .collect();
    let prepared_with = shared.prepare_metadata::<Reporter>(
        &mut discovered,
        &mut interpreters,
        &workspace.reachable_from(&needed),
    )
    .await?;
    workspace.update_manifests(&discovered);
    let mut selecting = Selecting {
        shared,
        workspace,
        manifests: discovered.into_iter().collect(),
        interpreters,
        prepared_with,
    };
    let mut units = Vec::new();
    for membership in memberships {
        units.push(selecting.unit::<Reporter>(membership).await?);
    }
    Ok(units)
}

/// What selecting the units of one install reads and looks at.
struct Selecting<'a> {
    shared: &'a Shared<'a>,
    workspace: workspace::Workspace,
    manifests: BTreeMap<PathBuf, Arc<manifest::Manifest>>,
    interpreters: Interpreters<'a>,
    /// The interpreter each project with dynamic metadata was prepared
    /// with, which a project on its own is then installed with too.
    prepared_with: BTreeMap<PathBuf, Arc<Interpreter>>,
}

impl Selecting<'_> {
    async fn unit<Reporter: self::Reporter + 'static>(
        &mut self,
        membership: Membership,
    ) -> Result<Unit> {
        let config = self.shared.context.config;
        let Membership { root, members, shared } = membership;
        let manifests = self.manifests_of(members);
        let interpreter = self.interpreter::<Reporter>(&root, &manifests).await?;
        let environments = Environments::of(config, &interpreter)?;
        self.workspace.for_resolution(config, &environments);
        let projects = Projects {
            local: Arc::from(self.local_projects(&manifests, &root)?),
            rules: self.rules(&root),
            members: self.members(manifests)?,
            root,
            shared,
        };
        Ok(Unit { interpreter, environments, projects })
    }

    fn manifests_of(&self, members: Vec<PathBuf>) -> Vec<(PathBuf, Arc<manifest::Manifest>)> {
        members
            .into_iter()
            .map(|member| {
                let manifest = self.manifests.get(&member).expect("a member was discovered");
                (member, Arc::clone(manifest))
            })
            .collect()
    }

    fn local_projects(
        &self,
        manifests: &[(PathBuf, Arc<manifest::Manifest>)],
        root: &Path,
    ) -> Result<Vec<workspace::LocalProject>> {
        let members = manifests
            .iter()
            .map(|(root, manifest)| (root.as_path(), &**manifest))
            .collect::<Vec<_>>();
        self.workspace.local_projects(&members, root)
    }

    /// The manifest whose overrides and constraints the unit resolves
    /// under: the workspace root's for a member, else the unit's own.
    fn rules(&self, root: &Path) -> Arc<manifest::Manifest> {
        let manifest = self.manifests.get(root).expect("a unit root was discovered");
        self.workspace.resolution_manifest(root, manifest)
    }

    fn members(&self, manifests: Vec<(PathBuf, Arc<manifest::Manifest>)>) -> Result<Vec<Member>> {
        manifests
            .into_iter()
            .map(|(root, manifest)| {
                let requirements =
                    Requirements::new(self.shared, &root, &manifest, &self.workspace)?;
                Ok(Member { root, manifest, requirements })
            })
            .collect()
    }

    /// The interpreter the members are installed with: one every member
    /// accepts. A project on its own keeps the one its dynamic metadata
    /// was prepared with, which its manifest already selected.
    async fn interpreter<Reporter: self::Reporter + 'static>(
        &mut self,
        root: &Path,
        members: &[(PathBuf, Arc<manifest::Manifest>)],
    ) -> Result<Arc<Interpreter>> {
        if let [(member, _)] = members
            && let Some(interpreter) = self.prepared_with.remove(member)
        {
            return Ok(interpreter);
        }
        let requires_python = interpreter::requires_python_of(
            members
                .iter()
                .map(|(root, manifest)| (root.as_path(), &**manifest)),
        )?;
        self.interpreters.select_accepting::<Reporter>(root, requires_python.as_ref()).await
    }
}

/// One lockfile and one environment, and the interpreter they are for.
struct Unit {
    interpreter: Arc<Interpreter>,
    environments: Environments,
    projects: Projects,
}

impl Unit {
    async fn prepare<Reporter: self::Reporter + 'static>(
        self,
        shared: &Shared<'_>,
    ) -> Result<Prepared> {
        PythonPrepare::for_project(shared, &self.interpreter, &self.environments)
            .projects::<Reporter>(self.projects)
            .await
    }
}

/// The projects one lockfile and one environment answer for.
pub(super) struct Projects {
    /// Where the lockfile and the environment live.
    pub(super) root: PathBuf,
    /// Whether the members asked to share, which the lockfile records.
    pub(super) shared: bool,
    pub(super) members: Vec<Member>,
    pub(super) local: Arc<[workspace::LocalProject]>,
    /// The manifest whose overrides and constraints the resolution reads.
    pub(super) rules: Arc<manifest::Manifest>,
}

impl Projects {
    /// The members as the lockfile records them, relative to it: the ones
    /// sharing the environment, and none for a project on its own.
    pub(super) fn recorded_members(&self) -> Vec<String> {
        if !self.shared {
            return Vec::new();
        }
        self.members
            .iter()
            .map(|member| workspace::relative(&self.root, &member.root))
            .collect()
    }
}

pub(super) struct Member {
    pub(super) root: PathBuf,
    pub(super) manifest: Arc<manifest::Manifest>,
    pub(super) requirements: Requirements,
}

impl Member {
    pub(super) fn requires_python(&self) -> Option<&str> {
        self.manifest.project.as_ref()?.requires_python.as_deref()
    }
}

/// The interpreter range the lockfile records: the one member's own
/// declaration, or the range every member accepts.
pub(super) fn requires_python_of(members: &[Member]) -> Result<Option<String>> {
    if let [member] = members {
        return Ok(member.requires_python().map(str::to_string));
    }
    let combined = interpreter::requires_python_of(
        members
            .iter()
            .map(|member| (member.root.as_path(), &*member.manifest)),
    )?;
    Ok(combined.map(|specifiers| specifiers.to_string()))
}

pub(super) struct Requirements {
    pub(super) all: Vec<pep508_rs::Requirement>,
    pub(super) selected: Vec<pep508_rs::Requirement>,
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
        })
    }

    /// What every member requires, together: one resolution answers for
    /// all of them.
    pub(super) fn merged(members: &[Member]) -> Self {
        let mut merged = Self { all: Vec::new(), selected: Vec::new() };
        for member in members {
            merged.all.extend(member.requirements.all.iter().cloned());
            merged.selected.extend(member.requirements.selected.iter().cloned());
        }
        merged
    }
}
