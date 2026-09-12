pub use report::render_release_plan;

use crate::cli_args::{
    changelog::{published_names, unpublished_release_dirs},
    recursive::discover_workspace_projects,
};
use clap::Args;
use derive_more::{Display, Error};
use dialoguer::{Input, MultiSelect};
use indexmap::IndexMap;
use miette::{Diagnostic, IntoDiagnostic};
use node_semver::Version;
use pnpm_config::Config;
use pnpm_package_manifest::DependencyGroup;
use pnpm_versioning::{
    AssembleReleasePlanOptions, IntentBumpType, ManifestDependency, ReleasePlan, VersioningError,
    VersioningSettings, WorkspaceProject, assemble_release_plan, check_versioning_invariants,
    index_project_refs, read_change_intents, read_ledger, to_project_dir, write_change_intent,
};
use pnpm_workspace::Project;
use pnpm_workspace_projects_filter::{GetChangedProjectsOptions, get_changed_projects};

use report::{render_status, run_check};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
};

/// `pnpm change` — record a change intent: which packages a change affects,
/// the bump type for each, and a summary that becomes the changelog entry.
/// The intent file is written to `.changeset/` in the changesets format.
#[derive(Debug, Args)]
pub struct ChangeArgs {
    /// `status` to print the pending intents and the release plan they
    /// produce, `check` to validate the committed versions against the
    /// configured epic bands and fixed-group lockstep; otherwise the packages
    /// the change affects.
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
    pub async fn run(self, config: &Config) -> miette::Result<()> {
        let Some(workspace_dir) = config.workspace_dir.clone() else {
            return Err(ChangeError::WorkspaceOnly.into());
        };
        let (projects, _) = discover_workspace_projects(&workspace_dir, config)?;
        let engine_projects = to_engine_projects(&projects);

        if self.run_diagnostic_form(&workspace_dir, &projects, &engine_projects, config).await? {
            return Ok(());
        }

        let releasable = releasable_projects(&engine_projects, &workspace_dir, &config.versioning);
        if releasable.is_empty() {
            return Err(ChangeError::NoPackages.into());
        }
        self.check_params_releasable(&releasable, &engine_projects, &workspace_dir)?;
        let bump = self
            .bump
            .as_ref()
            .map(|bump| {
                parse_bump(bump).ok_or_else(|| ChangeError::InvalidBump { bump: bump.clone() })
            })
            .transpose()?;

        // For a name shared by several projects the interactive picker offers
        // each project under its directory reference, so the written intent
        // stays unambiguous without the contributor knowing the rule exists.
        let pkg_refs = if self.params.is_empty() {
            let changed_dirs =
                detect_changed_dirs(&releasable, &engine_projects, &workspace_dir, config);
            prompt_for_packages(&releasable, &changed_dirs)?
        } else {
            self.params.clone()
        };

        let releases: IndexMap<String, IntentBumpType> = match bump {
            Some(bump) => pkg_refs.into_iter().map(|reference| (reference, bump)).collect(),
            None => prompt_bump_types(&pkg_refs)?,
        };

        let summary = self.change_summary()?;

        let id = write_change_intent(&workspace_dir, &releases, &summary)?;
        println!("Recorded change intent .changeset/{id}.md");
        Ok(())
    }
    fn change_summary(&self) -> miette::Result<String> {
        match &self.summary {
            Some(summary) => Ok(summary.clone()),
            None => Input::new()
                .with_prompt("Summary of the change (becomes the changelog entry)")
                .interact_text()
                .into_diagnostic(),
        }
    }

    /// Every reference in `params` must name a releasable project.
    fn check_params_releasable(
        &self,
        releasable: &[ReleasableProject],
        engine_projects: &[WorkspaceProject],
        workspace_dir: &Path,
    ) -> miette::Result<()> {
        let releasable_dirs: HashSet<&str> =
            releasable.iter().map(|project| project.dir.as_str()).collect();
        let refs = index_project_refs(engine_projects, workspace_dir);
        for reference in &self.params {
            check_reference_is_releasable(&refs, reference, &releasable_dirs)?;
        }
        Ok(())
    }

    /// Handle the `status` and `check` forms, reporting whether one ran.
    /// Only the exact no-option invocations are diagnostic, so a package
    /// that happens to be named "status" or "check" stays recordable.
    async fn run_diagnostic_form(
        &self,
        workspace_dir: &Path,
        projects: &[pnpm_workspace::Project],
        engine_projects: &[pnpm_versioning::WorkspaceProject],
        config: &Config,
    ) -> miette::Result<bool> {
        if self.params.len() != 1 || self.bump.is_some() || self.summary.is_some() {
            return Ok(false);
        }
        match self.params[0].as_str() {
            "status" => {
                let names = published_names(projects);
                let output = render_status(workspace_dir, engine_projects, &names, config).await?;
                println!("{output}");
            }
            "check" => run_check(workspace_dir, engine_projects, config)?,
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// Reject a package reference the workspace cannot record a change for:
/// one that names several projects, or none that is releasable.
fn check_reference_is_releasable(
    refs: &pnpm_versioning::ProjectRefIndex,
    reference: &str,
    releasable_dirs: &HashSet<&str>,
) -> Result<(), ChangeError> {
    let dirs = refs.ref_to_dirs(reference);
    if dirs.len() > 1 {
        return Err(ChangeError::AmbiguousPackage {
            reference: reference.to_owned(),
            dirs: dirs.into_iter().map(|dir| format!("./{dir}")).collect(),
        });
    }
    if dirs.first().is_none_or(|dir| !releasable_dirs.contains(dir.as_str())) {
        return Err(ChangeError::UnknownPackage { pkg_name: reference.to_owned() });
    }
    Ok(())
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
    engine_projects: &[WorkspaceProject],
    workspace_dir: &Path,
    config: &Config,
) -> HashSet<String> {
    let Some(base_commit) = detect_base_commit(workspace_dir) else {
        return HashSet::new();
    };
    let project_dirs: Vec<PathBuf> =
        engine_projects.iter().map(|project| project.root_dir.clone()).collect();
    let opts = GetChangedProjectsOptions {
        workspace_dir,
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

/// The merge-base of HEAD with the default branch, or `None`.
fn detect_base_commit(cwd: &Path) -> Option<String> {
    for branch in ["main", "master"] {
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
        let prompt = MultiSelect::new()
            .with_prompt(format!(
                "Which packages should have a {label} bump? (<space> to select, <enter> to confirm)",
            ))
            .items(&remaining);
        let picked = prompt.interact().into_diagnostic()?;
        let chosen: HashSet<usize> = picked.into_iter().collect();
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
                (DependencyGroup::Prod, pnpm_versioning::DependencyField::Dependencies),
                (DependencyGroup::Optional, pnpm_versioning::DependencyField::OptionalDependencies),
                (DependencyGroup::Peer, pnpm_versioning::DependencyField::PeerDependencies),
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

mod report;
