use super::{
    AssembleReleasePlanOptions, AutoExcludeRoot, Config, HashMap, HashSet, Host, Path, VersionArgs,
    VersionError, apply_release_plan, assemble_release_plan, changelog,
    confirmed_published_versions, discover_workspace_projects, is_git_repo, is_working_tree_clean,
    read_change_intents, read_ledger, render_release_plan, select_recursive_projects,
    to_engine_projects, unpublished_release_dirs,
};

/// The project dirs `--filter` selects, or `None` for an unfiltered run.
fn filtered_project_dirs(
    projects: &[pnpm_workspace::Project],
    config: &Config,
    workspace_dir: &Path,
) -> miette::Result<Option<HashSet<String>>> {
    if config.filter.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        selected_projects(projects, config, workspace_dir)?
            .into_iter()
            .map(|(_, dir)| dir)
            .collect::<HashSet<String>>(),
    ))
}

/// The projects the active `--filter` selectors pick, in graph order, as
/// `(name, workspace-relative dir)` pairs.
pub(crate) fn selected_projects(
    projects: &[pnpm_workspace::Project],
    config: &Config,
    workspace_dir: &Path,
) -> miette::Result<Vec<(Option<String>, String)>> {
    let selection =
        select_recursive_projects(projects, config, workspace_dir, AutoExcludeRoot::Disabled)?;
    Ok(selection.selected
        .iter()
        .map(|(root_dir, node)| {
            let name = node.package.project.manifest
                .value()
                .get("name")
                .and_then(|name| name.as_str())
                .map(ToString::to_string);
            (name, pnpm_versioning::to_project_dir(workspace_dir, root_dir))
        })
        .collect())
}

struct PlannedWorkspaceRelease {
    plan: pnpm_versioning::ReleasePlan,
    projects: Vec<pnpm_versioning::WorkspaceProject>,
    intents: Vec<pnpm_versioning::ChangeIntent>,
    published_names: HashMap<String, String>,
    private_dirs: HashSet<String>,
    unfiltered: bool,
}

struct WorkspaceReleaseInputs {
    intents: Vec<pnpm_versioning::ChangeIntent>,
    ledger: pnpm_versioning::Ledger,
    workspace_projects: Vec<pnpm_workspace::Project>,
    projects: Vec<pnpm_versioning::WorkspaceProject>,
    published_names: HashMap<String, String>,
    private_dirs: HashSet<String>,
}

async fn plan_workspace_release(
    config: &Config,
    workspace_dir: &Path,
) -> miette::Result<PlannedWorkspaceRelease> {
    let inputs = workspace_release_inputs(config, workspace_dir)?;
    let filter = filtered_project_dirs(&inputs.workspace_projects, config, workspace_dir)?;
    let plan =
        assemble_workspace_release_plan(config, workspace_dir, &inputs, filter.clone()).await?;

    Ok(PlannedWorkspaceRelease {
        plan,
        projects: inputs.projects,
        intents: inputs.intents,
        published_names: inputs.published_names,
        private_dirs: inputs.private_dirs,
        unfiltered: filter.is_none(),
    })
}

async fn assemble_workspace_release_plan(
    config: &Config,
    workspace_dir: &Path,
    inputs: &WorkspaceReleaseInputs,
    filter: Option<HashSet<String>>,
) -> miette::Result<pnpm_versioning::ReleasePlan> {
    let assemble = |unpublished_dirs: HashSet<String>| {
        assemble_release_plan(
            &inputs.projects,
            workspace_dir,
            &inputs.intents,
            &inputs.ledger,
            Some(&config.versioning),
            &AssembleReleasePlanOptions {
                filter: filter.clone(),
                snapshot_suffix: None,
                enforce_workspace_protocol: true,
                unpublished_dirs,
            },
        )
    };
    let unpublished_dirs = unpublished_release_dirs(
        &assemble(HashSet::new())?,
        &changelog::ReleaseRegistryOptions {
            config,
            workspace_dir,
            published_names: &inputs.published_names,
            private_dirs: &inputs.private_dirs,
        },
    )
    .await?;
    Ok(assemble(unpublished_dirs)?)
}

fn workspace_release_inputs(
    config: &Config,
    workspace_dir: &Path,
) -> miette::Result<WorkspaceReleaseInputs> {
    let intents = read_change_intents(workspace_dir)?;
    let ledger = read_ledger(workspace_dir)?;
    let (projects, _) = discover_workspace_projects(workspace_dir, config)?;
    let engine_projects = to_engine_projects(&projects);
    let published_names = changelog::published_names(&projects);
    let private_dirs = changelog::private_project_dirs(&projects, workspace_dir);
    Ok(WorkspaceReleaseInputs {
        intents,
        ledger,
        workspace_projects: projects,
        projects: engine_projects,
        published_names,
        private_dirs,
    })
}

impl PlannedWorkspaceRelease {
    async fn confirmed_published_versions(
        &self,
        config: &Config,
        workspace_dir: &Path,
    ) -> miette::Result<HashSet<String>> {
        confirmed_published_versions(
            &changelog::ReleaseRegistryOptions {
                config,
                workspace_dir,
                published_names: &self.published_names,
                private_dirs: &self.private_dirs,
            },
            &self.projects,
        )
        .await
    }

    pub(super) async fn apply(
        self,
        args: &VersionArgs,
        config: &Config,
        workspace_dir: &Path,
    ) -> miette::Result<()> {
        if self.plan.releases.is_empty() {
            // A full (unfiltered) run garbage-collects the intent files an
            // empty plan leaves behind: declined ("none"-only) intents and
            // files a merge resurrected after every named package had already
            // consumed them. A filtered run must not — "nothing pending in
            // this scope" is no reason to delete prose belonging to packages
            // outside the filter.
            if !args.dry_run && self.unfiltered {
                let confirmed = self.confirmed_published_versions(config, workspace_dir).await?;
                apply_release_plan(
                    &self.plan,
                    workspace_dir,
                    &self.projects,
                    &self.intents,
                    Some(&config.versioning),
                    &confirmed,
                )?;
            }
            args.report_no_pending_changes();
            return Ok(());
        }
        if args.dry_run {
            println!("{}", render_release_plan(&self.plan));
            return Ok(());
        }

        let confirmed = self.confirmed_published_versions(config, workspace_dir).await?;
        let Self {
            plan,
            projects: engine_projects,
            intents,
            ..
        } = self;
        let applied = apply_release_plan(
            &plan,
            workspace_dir,
            &engine_projects,
            &intents,
            Some(&config.versioning),
            &confirmed,
        )?;

        args.report_applied_releases(&applied);
        Ok(())
    }
}

impl VersionArgs {
    pub(super) async fn release_from_intents(&self, config: &Config) -> miette::Result<()> {
        let Some(workspace_dir) = config.workspace_dir.clone() else {
            return Err(VersionError::ReleaseOutsideWorkspace.into());
        };

        if !self.dry_run
            && config.git_checks
            && !self.git.no_git_checks
            && is_git_repo::<Host>(&workspace_dir)
            && !is_working_tree_clean::<Host>(&workspace_dir)
        {
            return Err(VersionError::UncleanWorkingTree.into());
        }

        plan_workspace_release(config, &workspace_dir).await?.apply(self, config, &workspace_dir)
            .await
    }

    fn report_no_pending_changes(&self) {
        if self.json {
            println!("[]");
        } else {
            println!(r#"No pending changes. Record one with "pnpm change"."#);
        }
    }

    fn report_applied_releases(&self, applied: &[pnpm_versioning::AppliedRelease]) {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(applied).expect("serialize applied releases"),
            );
            return;
        }

        use std::fmt::Write as _;
        let mut output = String::from("Versions applied:\n");
        for release in applied {
            writeln!(
                output,
                "{}: {} → {}",
                release.name, release.current_version, release.new_version,
            )
            .expect("write to string");
        }
        println!("{output}");
    }
}
