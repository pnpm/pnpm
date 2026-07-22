use clap::Args;
use derive_more::{Display, Error};
use dialoguer::{Input, MultiSelect};
use indexmap::IndexMap;
use miette::{Diagnostic, IntoDiagnostic};
use node_semver::Version;
use pacquet_config::Config;
use pacquet_package_manifest::DependencyGroup;
use pacquet_versioning::{
    AssembleReleasePlanOptions, ChangelogSettings, ChangelogStorage, IntentBumpType,
    ManifestDependency, ReleasePlan, VersioningSettings, WorkspaceProject, assemble_release_plan,
    build_consumption_index, index_project_refs, read_change_intents, read_ledger, to_project_dir,
    write_change_intent,
};
use pacquet_workspace::Project;
use pacquet_workspace_projects_filter::{
    GetChangedProjectsOptions, collect_catalog_users, get_changed_projects,
};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::cli_args::{
    changelog::unpublished_release_dirs, recursive::discover_workspace_projects,
};

/// `pnpm change` — record a change intent: which packages a change affects,
/// the bump type for each, and a summary that becomes the changelog entry.
/// The intent file is written to `.changeset/` in the changesets format.
#[derive(Debug, Args)]
pub struct ChangeArgs {
    /// `status` prints the pending release plan; `check [since]` verifies
    /// changed-package coverage; `migrate` imports Changesets configuration;
    /// otherwise these are the packages the change affects.
    pub params: Vec<String>,

    /// Bump type for the named packages: none, patch, minor, major. "none"
    /// records an explicit decline — the change needs no release.
    #[clap(long)]
    pub bump: Option<String>,

    /// The summary for the changelog entry. Runs non-interactively when
    /// given together with package names.
    #[clap(long)]
    pub summary: Option<String>,
}

/// Errors of `pnpm change`. Codes and messages match the TypeScript CLI.
#[derive(Debug, Display, Error, Diagnostic)]
enum ChangeError {
    #[display("pnpm change is only supported in a workspace")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_ONLY))]
    WorkspaceOnly,

    #[display("No releasable packages found in this workspace")]
    #[diagnostic(code(ERR_PNPM_VERSIONING_NO_PACKAGES))]
    NoPackages,

    #[display("{pkg_name} is not a releasable package of this workspace")]
    #[diagnostic(code(ERR_PNPM_VERSIONING_UNKNOWN_PACKAGE))]
    UnknownPackage { pkg_name: String },

    #[display(
        "{reference} matches multiple workspace projects: {}. Reference the project by directory instead.",
        dirs.join(", ")
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_AMBIGUOUS_PACKAGE))]
    AmbiguousPackage { reference: String, dirs: Vec<String> },

    #[display("Invalid bump type: {bump}. Expected one of none, patch, minor, major")]
    #[diagnostic(code(ERR_PNPM_VERSIONING_INVALID_BUMP))]
    InvalidBump { bump: String },

    #[display("pnpm change check accepts at most one base revision")]
    #[diagnostic(code(ERR_PNPM_VERSIONING_INVALID_CHECK_ARGS))]
    InvalidCheckArgs,

    #[display(
        "Could not find a base revision for pnpm change check. Pass one explicitly, for example: pnpm change check origin/main"
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_BASE_NOT_FOUND))]
    BaseNotFound,

    #[display(
        "Changed packages are missing a pending change intent: {}. Record a bump or an explicit none decline with pnpm change.",
        packages.join(", ")
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_CHANGE_CHECK_FAILED))]
    CheckFailed { packages: Vec<String> },

    #[display("No Changesets config found at {}", path.display())]
    #[diagnostic(code(ERR_PNPM_VERSIONING_CHANGESETS_CONFIG_NOT_FOUND))]
    ChangesetsConfigNotFound { path: PathBuf },

    #[display("Cannot parse {}: {message}", path.display())]
    #[diagnostic(code(ERR_PNPM_VERSIONING_INVALID_CHANGESETS_CONFIG))]
    InvalidChangesetsConfig { path: PathBuf, message: String },

    #[display(
        "Cannot migrate .changeset/config.json because linked groups are not supported. Convert them to fixed groups or remove them first."
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_LINKED_UNSUPPORTED))]
    LinkedUnsupported,
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct ChangesetsConfig {
    fixed: Option<Vec<Vec<String>>>,
    ignore: Option<Vec<String>>,
    linked: Option<Vec<Vec<String>>>,
}

impl ChangeArgs {
    pub async fn run(self, config: &Config) -> miette::Result<()> {
        let Some(workspace_dir) = config.workspace_dir.clone() else {
            return Err(ChangeError::WorkspaceOnly.into());
        };
        let (projects, _) = discover_workspace_projects(&workspace_dir)?;
        let engine_projects = to_engine_projects(&projects);

        if self.params.len() == 1
            && self.params[0] == "migrate"
            && self.bump.is_none()
            && self.summary.is_none()
        {
            let repository_changelog =
                migrate_changesets_config(&workspace_dir, &projects, &config.versioning)?;
            println!(
                "Migrated .changeset/config.json to pnpm-workspace.yaml{}.",
                if repository_changelog { " with repository changelog storage" } else { "" },
            );
            return Ok(());
        }

        if self.params.first().is_some_and(|param| param == "check")
            && self.bump.is_none()
            && self.summary.is_none()
        {
            check_change_coverage(
                &workspace_dir,
                &projects,
                &engine_projects,
                config,
                &self.params[1..],
            )?;
            println!("All changed packages are covered by change intents.");
            return Ok(());
        }

        // Only the exact no-option invocation is the status form, so a
        // package that happens to be named "status" stays recordable.
        if self.params.len() == 1
            && self.params[0] == "status"
            && self.bump.is_none()
            && self.summary.is_none()
        {
            let output = render_status(&workspace_dir, &engine_projects, config).await?;
            println!("{output}");
            return Ok(());
        }

        let releasable = releasable_projects(&engine_projects, &workspace_dir, &config.versioning);
        if releasable.is_empty() {
            return Err(ChangeError::NoPackages.into());
        }
        let releasable_dirs: HashSet<&str> =
            releasable.iter().map(|project| project.dir.as_str()).collect();
        let refs = index_project_refs(&engine_projects, &workspace_dir);
        for reference in &self.params {
            let dirs = refs.ref_to_dirs(reference);
            if dirs.len() > 1 {
                return Err(ChangeError::AmbiguousPackage {
                    reference: reference.clone(),
                    dirs: dirs.into_iter().map(|dir| format!("./{dir}")).collect(),
                }
                .into());
            }
            if dirs.first().is_none_or(|dir| !releasable_dirs.contains(dir.as_str())) {
                return Err(ChangeError::UnknownPackage { pkg_name: reference.clone() }.into());
            }
        }
        let bump = match &self.bump {
            None => None,
            Some(bump) => match parse_bump(bump) {
                Some(parsed) => Some(parsed),
                None => return Err(ChangeError::InvalidBump { bump: bump.clone() }.into()),
            },
        };

        // For a name shared by several projects the interactive picker offers
        // each project under its directory reference, so the written intent
        // stays unambiguous without the contributor knowing the rule exists.
        let pkg_refs = if self.params.is_empty() {
            let changed_dirs = detect_changed_dirs(
                &releasable,
                &projects,
                &engine_projects,
                &workspace_dir,
                config,
            );
            prompt_for_packages(&releasable, &changed_dirs)?
        } else {
            self.params.clone()
        };

        let releases: IndexMap<String, IntentBumpType> = match bump {
            Some(bump) => pkg_refs.into_iter().map(|reference| (reference, bump)).collect(),
            None => prompt_bump_types(&pkg_refs)?,
        };

        let summary = match &self.summary {
            Some(summary) => summary.clone(),
            None => Input::new()
                .with_prompt("Summary of the change (becomes the changelog entry)")
                .interact_text()
                .into_diagnostic()?,
        };

        let id = write_change_intent(&workspace_dir, &releases, &summary)?;
        println!("Recorded change intent .changeset/{id}.md");
        Ok(())
    }
}

fn migrate_changesets_config(
    workspace_dir: &Path,
    projects: &[Project],
    current: &VersioningSettings,
) -> miette::Result<bool> {
    let config_path = workspace_dir.join(".changeset/config.json");
    let text = match fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ChangeError::ChangesetsConfigNotFound { path: config_path }.into());
        }
        Err(err) => return Err(err).into_diagnostic(),
    };
    let legacy: ChangesetsConfig = serde_json::from_str(&text).map_err(|err| {
        ChangeError::InvalidChangesetsConfig { path: config_path.clone(), message: err.to_string() }
    })?;
    if legacy.linked.as_ref().is_some_and(|groups| !groups.is_empty()) {
        return Err(ChangeError::LinkedUnsupported.into());
    }
    let repository_changelog =
        projects.iter().any(|project| project.root_dir.join("CHANGELOG.md").is_file());
    let mut versioning = current.clone();
    if let Some(fixed) = legacy.fixed {
        versioning.fixed = fixed;
    }
    if let Some(ignore) = legacy.ignore {
        versioning.ignore = ignore;
    }
    if repository_changelog {
        versioning.changelog = Some(ChangelogSettings {
            storage: Some(ChangelogStorage::Repository),
            ..versioning.changelog.unwrap_or_default()
        });
    }
    let value = serde_json::to_value(&versioning).into_diagnostic()?;
    pacquet_workspace_manifest_writer::update_manifest_field(
        &workspace_dir.join("pnpm-workspace.yaml"),
        "versioning",
        &value,
    )?;
    fs::remove_file(&config_path).into_diagnostic()?;
    Ok(repository_changelog)
}

fn parse_bump(bump: &str) -> Option<IntentBumpType> {
    match bump {
        "none" => Some(IntentBumpType::None),
        "patch" => Some(IntentBumpType::Patch),
        "minor" => Some(IntentBumpType::Minor),
        "major" => Some(IntentBumpType::Major),
        _ => None,
    }
}

/// The affected-packages picker. The packages whose directories the branch
/// touched come first and are preselected; the rest follow. (dialoguer has no
/// section headings, so the grouping is conveyed by order and preselection
/// rather than the "changed packages" / "unchanged packages" separators the
/// TypeScript CLI's inquirer prompt renders.) A name shared by several
/// projects is offered under its directory reference so the written intent
/// stays unambiguous.
fn prompt_for_packages(
    releasable: &[ReleasableProject],
    changed_dirs: &HashSet<String>,
) -> miette::Result<Vec<String>> {
    let mut ordered: Vec<&ReleasableProject> =
        releasable.iter().filter(|project| changed_dirs.contains(&project.dir)).collect();
    ordered.extend(releasable.iter().filter(|project| !changed_dirs.contains(&project.dir)));

    let items: Vec<(String, bool)> = ordered
        .iter()
        .map(|project| {
            let label = if project.reference == project.name {
                project.name.clone()
            } else {
                format!("{} (./{})", project.name, project.dir)
            };
            (label, changed_dirs.contains(&project.dir))
        })
        .collect();

    loop {
        let indices = MultiSelect::new()
            .with_prompt(
                "Which packages does this change affect? (<space> to select, <enter> to confirm)",
            )
            .items_checked(items.iter().cloned())
            .interact()
            .into_diagnostic()?;
        if !indices.is_empty() {
            return Ok(indices.into_iter().map(|index| ordered[index].reference.clone()).collect());
        }
        println!("Select at least one package.");
    }
}

/// The workspace-relative directories the current branch changed, relative to
/// the base branch, using the same detection behind `--filter="[<ref>]"`.
/// Returns an empty set on any failure so the picker degrades to a flat list.
fn detect_changed_dirs(
    releasable: &[ReleasableProject],
    projects: &[Project],
    engine_projects: &[WorkspaceProject],
    workspace_dir: &Path,
    config: &Config,
) -> HashSet<String> {
    let Some(base_commit) = detect_base_commit(workspace_dir) else {
        return HashSet::new();
    };
    let project_dirs: Vec<PathBuf> =
        engine_projects.iter().map(|project| project.root_dir.clone()).collect();
    let catalog_users = collect_project_catalog_users(projects);
    let opts = GetChangedProjectsOptions {
        workspace_dir,
        workspace_root: workspace_dir,
        catalog_users: &catalog_users,
        test_pattern: &config.test_pattern,
        changed_files_ignore_pattern: &config.changed_files_ignore_pattern,
    };
    let Ok(changed) = get_changed_projects(project_dirs, &base_commit, &opts) else {
        return HashSet::new();
    };
    let releasable_dirs: HashSet<&str> =
        releasable.iter().map(|project| project.dir.as_str()).collect();
    changed
        .changed_projects
        .iter()
        .map(|root_dir| to_project_dir(workspace_dir, root_dir))
        .filter(|dir| releasable_dirs.contains(dir.as_str()))
        .collect()
}

fn check_change_coverage(
    workspace_dir: &Path,
    projects: &[Project],
    engine_projects: &[WorkspaceProject],
    config: &Config,
    params: &[String],
) -> miette::Result<()> {
    if params.len() > 1 {
        return Err(ChangeError::InvalidCheckArgs.into());
    }
    let base_commit = match params.first() {
        Some(commit) => commit.clone(),
        None => detect_base_commit(workspace_dir).ok_or(ChangeError::BaseNotFound)?,
    };
    let releasable = releasable_projects(engine_projects, workspace_dir, &config.versioning);
    let releasable_by_dir: std::collections::HashMap<&str, &ReleasableProject> =
        releasable.iter().map(|project| (project.dir.as_str(), project)).collect();
    let catalog_users = collect_project_catalog_users(projects.iter());
    let changed = get_changed_projects(
        projects.iter().map(|project| project.root_dir.clone()).collect(),
        &base_commit,
        &GetChangedProjectsOptions {
            workspace_dir,
            workspace_root: workspace_dir,
            catalog_users: &catalog_users,
            test_pattern: &config.test_pattern,
            changed_files_ignore_pattern: &config.changed_files_ignore_pattern,
        },
    )?;
    let touched: Vec<String> = changed
        .changed_projects
        .iter()
        .map(|root_dir| to_project_dir(workspace_dir, root_dir))
        .filter(|dir| releasable_by_dir.contains_key(dir.as_str()))
        .collect();
    if touched.is_empty() {
        return Ok(());
    }

    let refs = index_project_refs(engine_projects, workspace_dir);
    let intents = read_change_intents(workspace_dir)?;
    let ledger = read_ledger(workspace_dir)?;
    let consumption = build_consumption_index(&ledger, |name| refs.name_to_dirs(name))?;
    let mut covered = HashSet::new();
    for intent in &intents {
        for reference in intent.releases.keys() {
            for dir in refs.ref_to_dirs(reference) {
                let already_consumed =
                    consumption.get(&dir).is_some_and(|entry| entry.all_ids.contains(&intent.id));
                if !already_consumed {
                    covered.insert(dir);
                }
            }
        }
    }
    let mut uncovered: Vec<String> = touched
        .iter()
        .filter(|dir| !covered.contains(*dir))
        .map(|dir| releasable_by_dir[dir.as_str()].reference.clone())
        .collect();
    uncovered.sort();
    if !uncovered.is_empty() {
        return Err(ChangeError::CheckFailed { packages: uncovered }.into());
    }
    Ok(())
}

fn collect_project_catalog_users<'a>(
    projects: impl IntoIterator<Item = &'a Project>,
) -> pacquet_workspace_projects_filter::CatalogUsers {
    collect_catalog_users(projects.into_iter().map(|project| {
        let dependencies = project
            .manifest
            .dependencies([
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
                DependencyGroup::Peer,
            ])
            .map(|(name, specifier)| (name.to_string(), specifier.to_string()))
            .collect();
        (project.root_dir.clone(), dependencies)
    }))
}

/// The merge-base of HEAD with the default branch, or `None`.
fn detect_base_commit(cwd: &Path) -> Option<String> {
    for branch in ["origin/main", "main", "origin/master", "master"] {
        let Ok(output) =
            Command::new("git").args(["merge-base", "HEAD", branch]).current_dir(cwd).output()
        else {
            continue;
        };
        if output.status.success() {
            let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !commit.is_empty() {
                return Some(commit);
            }
        }
    }
    None
}

/// The changesets-style bump picker: ask which packages get a major bump,
/// then which of the rest get a minor, and default whatever remains to patch.
/// One multiselect per level reads far better than a per-package prompt when
/// many packages are affected. Bumps are returned in the original selection
/// order rather than grouped by level.
fn prompt_bump_types(pkg_refs: &[String]) -> miette::Result<IndexMap<String, IntentBumpType>> {
    let mut bump_by_ref: IndexMap<String, IntentBumpType> = IndexMap::new();
    let mut remaining: Vec<String> = pkg_refs.to_vec();
    for (label, bump_type) in [("major", IntentBumpType::Major), ("minor", IntentBumpType::Minor)] {
        if remaining.is_empty() {
            break;
        }
        let chosen: HashSet<usize> = MultiSelect::new()
            .with_prompt(format!(
                "Which packages should have a {label} bump? (<space> to select, <enter> to confirm)",
            ))
            .items(&remaining)
            .interact()
            .into_diagnostic()?
            .into_iter()
            .collect();
        let mut next_remaining = Vec::new();
        for (index, reference) in remaining.into_iter().enumerate() {
            if chosen.contains(&index) {
                bump_by_ref.insert(reference, bump_type);
            } else {
                next_remaining.push(reference);
            }
        }
        remaining = next_remaining;
    }
    for reference in remaining {
        bump_by_ref.insert(reference, IntentBumpType::Patch);
    }
    Ok(pkg_refs.iter().map(|reference| (reference.clone(), bump_by_ref[reference])).collect())
}

async fn render_status(
    workspace_dir: &Path,
    projects: &[WorkspaceProject],
    config: &Config,
) -> miette::Result<String> {
    let intents = read_change_intents(workspace_dir)?;
    let ledger = read_ledger(workspace_dir)?;
    let assemble = |unpublished_dirs: HashSet<String>| {
        assemble_release_plan(
            projects,
            workspace_dir,
            &intents,
            &ledger,
            Some(&config.versioning),
            &AssembleReleasePlanOptions { unpublished_dirs, ..Default::default() },
        )
    };
    // Probe as the release does, so the preview matches it.
    let unpublished_dirs = unpublished_release_dirs(config, &assemble(HashSet::new())?).await?;
    let plan = assemble(unpublished_dirs)?;
    if plan.releases.is_empty() {
        return Ok("No pending changes.".to_string());
    }
    let consumed_ids: std::collections::HashSet<&str> = plan
        .releases
        .iter()
        .flat_map(|release| release.intents.iter().map(|intent| intent.id.as_str()))
        .collect();
    use std::fmt::Write as _;
    let mut output = String::from("Pending change intents:\n");
    for intent in intents.iter().filter(|intent| consumed_ids.contains(intent.id.as_str())) {
        writeln!(output, "  .changeset/{}.md", intent.id).expect("write to string");
    }
    output.push('\n');
    output.push_str(&render_release_plan(&plan));
    Ok(output)
}

/// Renders the plan the way the TypeScript CLI prints it, one line per
/// release.
pub fn render_release_plan(plan: &ReleasePlan) -> String {
    use std::fmt::Write as _;
    let mut output = String::from("Release plan:\n");
    for release in &plan.releases {
        let causes: Vec<String> = release.causes.iter().map(ToString::to_string).collect();
        writeln!(
            output,
            "  {}: {} → {} ({}, via {})",
            release.name,
            release.current_version,
            release.new_version,
            release.bump_type,
            causes.join("+"),
        )
        .expect("write to string");
    }
    output
}

/// One project `pnpm change` may record an intent for, and how an intent
/// file or versioning config should reference it: the bare name, or the
/// `./`-prefixed directory when the name is shared by several workspace
/// projects.
pub struct ReleasableProject {
    pub name: String,
    /// Workspace-relative project directory.
    pub dir: String,
    pub reference: String,
}

/// The projects a change intent may demand a release from: named, carrying a
/// valid semver version, and not frozen by `versioning.ignore`. Matches the
/// participant set of the release-plan assembler.
pub fn releasable_projects(
    projects: &[WorkspaceProject],
    workspace_dir: &Path,
    versioning: &VersioningSettings,
) -> Vec<ReleasableProject> {
    let refs = index_project_refs(projects, workspace_dir);
    let ignored_dirs: HashSet<String> =
        versioning.ignore.iter().flat_map(|reference| refs.ref_to_dirs(reference)).collect();
    let mut releasable: Vec<ReleasableProject> = projects
        .iter()
        .filter_map(|project| {
            let (Some(name), Some(version)) = (&project.name, &project.version) else {
                return None;
            };
            if Version::parse(version).is_err() {
                return None;
            }
            let dir = to_project_dir(workspace_dir, &project.root_dir);
            if ignored_dirs.contains(&dir) {
                return None;
            }
            let reference =
                if refs.name_to_dirs(name).len() > 1 { format!("./{dir}") } else { name.clone() };
            Some(ReleasableProject { name: name.clone(), dir, reference })
        })
        .collect();
    releasable.sort_by(|left, right| left.reference.cmp(&right.reference));
    releasable
}

/// Extracts the manifest fields the release-plan assembler consumes from the
/// discovered workspace projects.
pub fn to_engine_projects(projects: &[Project]) -> Vec<WorkspaceProject> {
    projects
        .iter()
        .map(|project| {
            let manifest = project.manifest.value();
            let mut prod_dependencies = Vec::new();
            for (group, field) in [
                (DependencyGroup::Prod, pacquet_versioning::DependencyField::Dependencies),
                (
                    DependencyGroup::Optional,
                    pacquet_versioning::DependencyField::OptionalDependencies,
                ),
                (DependencyGroup::Peer, pacquet_versioning::DependencyField::PeerDependencies),
            ] {
                for (alias, spec) in project.manifest.dependencies([group]) {
                    prod_dependencies.push(ManifestDependency {
                        field,
                        alias: alias.to_string(),
                        spec: spec.to_string(),
                    });
                }
            }
            WorkspaceProject {
                root_dir: project.root_dir.clone(),
                name: manifest.get("name").and_then(|name| name.as_str()).map(ToString::to_string),
                version: manifest
                    .get("version")
                    .and_then(|version| version.as_str())
                    .map(ToString::to_string),
                prod_dependencies,
            }
        })
        .collect()
}
