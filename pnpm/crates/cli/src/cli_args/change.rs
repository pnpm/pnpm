use clap::Args;
use derive_more::{Display, Error};
use dialoguer::{Input, MultiSelect, Select};
use indexmap::IndexMap;
use miette::{Diagnostic, IntoDiagnostic};
use node_semver::Version;
use pacquet_config::Config;
use pacquet_package_manifest::DependencyGroup;
use pacquet_versioning::{
    AssembleReleasePlanOptions, IntentBumpType, ManifestDependency, ReleasePlan,
    VersioningSettings, WorkspaceProject, assemble_release_plan, index_project_refs,
    read_change_intents, read_ledger, to_project_dir, write_change_intent,
};
use pacquet_workspace::Project;
use std::{collections::HashSet, path::Path};

use crate::cli_args::recursive::discover_workspace_projects;

/// `pnpm change` — record a change intent: which packages a change affects,
/// the bump type for each, and a summary that becomes the changelog entry.
/// The intent file is written to `.changeset/` in the changesets format.
#[derive(Debug, Args)]
pub struct ChangeArgs {
    /// `status` to print the pending intents and the release plan they
    /// produce; otherwise the packages the change affects.
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
}

impl ChangeArgs {
    pub fn run(self, config: &Config) -> miette::Result<()> {
        let Some(workspace_dir) = config.workspace_dir.clone() else {
            return Err(ChangeError::WorkspaceOnly.into());
        };
        let (projects, _) = discover_workspace_projects(&workspace_dir)?;
        let engine_projects = to_engine_projects(&projects);

        // Only the exact no-option invocation is the status form, so a
        // package that happens to be named "status" stays recordable.
        if self.params.len() == 1
            && self.params[0] == "status"
            && self.bump.is_none()
            && self.summary.is_none()
        {
            let output = render_status(&workspace_dir, &engine_projects, config)?;
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
            prompt_for_packages(&releasable)?
        } else {
            self.params.clone()
        };

        let mut releases: IndexMap<String, IntentBumpType> = IndexMap::new();
        for reference in pkg_refs {
            let bump_type = match bump {
                Some(bump) => bump,
                None => prompt_for_bump(&reference)?,
            };
            releases.insert(reference, bump_type);
        }

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

fn parse_bump(bump: &str) -> Option<IntentBumpType> {
    match bump {
        "none" => Some(IntentBumpType::None),
        "patch" => Some(IntentBumpType::Patch),
        "minor" => Some(IntentBumpType::Minor),
        "major" => Some(IntentBumpType::Major),
        _ => None,
    }
}

fn prompt_for_packages(releasable: &[ReleasableProject]) -> miette::Result<Vec<String>> {
    let labels: Vec<String> = releasable
        .iter()
        .map(|project| {
            if project.reference == project.name {
                project.name.clone()
            } else {
                format!("{} (./{})", project.name, project.dir)
            }
        })
        .collect();
    loop {
        let indices = MultiSelect::new()
            .with_prompt(
                "Which packages does this change affect? (<space> to select, <enter> to confirm)",
            )
            .items(&labels)
            .interact()
            .into_diagnostic()?;
        if !indices.is_empty() {
            return Ok(indices
                .into_iter()
                .map(|index| releasable[index].reference.clone())
                .collect());
        }
        println!("Select at least one package.");
    }
}

fn prompt_for_bump(pkg_name: &str) -> miette::Result<IntentBumpType> {
    const CHOICES: [IntentBumpType; 4] =
        [IntentBumpType::Major, IntentBumpType::Minor, IntentBumpType::Patch, IntentBumpType::None];
    let index = Select::new()
        .with_prompt(format!("Bump type for {pkg_name}"))
        .items(CHOICES.map(|choice| choice.to_string()))
        .default(2)
        .interact()
        .into_diagnostic()?;
    Ok(CHOICES[index])
}

fn render_status(
    workspace_dir: &Path,
    projects: &[WorkspaceProject],
    config: &Config,
) -> miette::Result<String> {
    let intents = read_change_intents(workspace_dir)?;
    let ledger = read_ledger(workspace_dir)?;
    let plan = assemble_release_plan(
        projects,
        workspace_dir,
        &intents,
        &ledger,
        Some(&config.versioning),
        &AssembleReleasePlanOptions::default(),
    )?;
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
