use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_publish::{Host, is_git_repo, is_working_tree_clean};
use pacquet_versioning::{
    ApplyReleasePlanOptions, AssembleReleasePlanOptions, VersioningSettings, apply_release_plan,
    assemble_release_plan, read_change_intents, read_ledger,
};
use pacquet_workspace_manifest_writer::update_manifest_field;
use std::{collections::HashSet, path::Path};

use crate::cli_args::{
    change::{releasable_pkg_names, render_release_plan, to_engine_projects},
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
};

/// `pnpm version` — apply the pending change intents (`-r` with no version
/// argument) or manage per-package prerelease lines (`stable` / `unstable`).
/// The npm-style explicit-bump forms are not ported yet.
#[derive(Debug, Args)]
pub struct VersionArgs {
    /// `unstable <tag>` / `stable` to manage prerelease lines; an npm-style
    /// version argument otherwise.
    pub params: Vec<String>,

    /// Print the release plan the pending change intents produce without
    /// applying it.
    #[clap(long = "dry-run")]
    pub dry_run: bool,

    /// Release one-off snapshot versions (0.0.0-<tag>-<timestamp>) without
    /// consuming change intents. Intended for CI preview publishing:
    /// publish, then discard the manifest changes.
    #[clap(long, num_args = 0..=1, default_missing_value = "")]
    pub snapshot: Option<String>,

    /// Don't check if the working tree is clean.
    #[clap(long = "no-git-checks")]
    pub no_git_checks: bool,
}

/// Errors of `pnpm version`. Codes and messages match the TypeScript CLI.
#[derive(Debug, Display, Error, Diagnostic)]
enum VersionError {
    #[display(
        "A version argument is required. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_VERSION_BUMP))]
    MissingBump,

    #[display(
        "The npm-style \"pnpm version {bump}\" form is not implemented in the Rust CLI yet. The bare \"pnpm version -r\" form that consumes change intents is available."
    )]
    #[diagnostic(code(ERR_PNPM_NOT_IMPLEMENTED))]
    NpmStyleNotPorted { bump: String },

    #[display(
        r#"The bare "pnpm version -r" form consumes change intents and is only supported in a workspace"#
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_ONLY))]
    ReleaseOutsideWorkspace,

    #[display(
        "\"pnpm version {action}\" manages per-package prerelease lines and is only supported in a workspace"
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_ONLY))]
    PrereleaseLineOutsideWorkspace { action: String },

    #[display("Working tree is not clean. Commit or stash your changes.")]
    #[diagnostic(code(ERR_PNPM_UNCLEAN_WORKING_TREE))]
    UncleanWorkingTree,

    #[display(
        "Select the packages to move with --filter, e.g. \"pnpm version {example} --filter <pkg>...\""
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_PRE_FILTER_REQUIRED))]
    FilterRequired { example: String },

    #[display("The filter selected no releasable packages")]
    #[diagnostic(code(ERR_PNPM_VERSIONING_NO_PACKAGES))]
    NoPackagesSelected,

    #[display(
        r#"A prerelease tag is required, e.g. "pnpm version unstable alpha". Tags may contain only alphanumerics and hyphens, and cannot be purely numeric."#
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_INVALID_PRERELEASE_TAG))]
    InvalidPrereleaseTag,

    #[display(
        "{pkg_name} is already on the \"{tag}\" prerelease line. Move it back with \"pnpm version stable\" first."
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_ALREADY_ON_LINE))]
    AlreadyOnLine { pkg_name: String, tag: String },
}

impl VersionArgs {
    pub fn run(self, config: &Config, recursive: bool) -> miette::Result<()> {
        match self.params.first().map(String::as_str) {
            Some(action @ ("stable" | "unstable")) => {
                let action = action.to_string();
                self.handle_prerelease_line(config, &action)
            }
            None if recursive => self.release_from_intents(config),
            None => Err(VersionError::MissingBump.into()),
            Some(bump) => Err(VersionError::NpmStyleNotPorted { bump: bump.to_string() }.into()),
        }
    }

    fn release_from_intents(&self, config: &Config) -> miette::Result<()> {
        let Some(workspace_dir) = config.workspace_dir.clone() else {
            return Err(VersionError::ReleaseOutsideWorkspace.into());
        };

        if !self.dry_run
            && config.git_checks
            && !self.no_git_checks
            && is_git_repo::<Host>(&workspace_dir)
            && !is_working_tree_clean::<Host>(&workspace_dir)
        {
            return Err(VersionError::UncleanWorkingTree.into());
        }

        let intents = read_change_intents(&workspace_dir)?;
        let ledger = read_ledger(&workspace_dir)?;
        let (projects, _) = discover_workspace_projects(&workspace_dir)?;
        let engine_projects = to_engine_projects(&projects);

        let filter = if config.filter.is_empty() {
            None
        } else {
            Some(
                selected_pkg_names(&projects, config, &workspace_dir)?
                    .into_iter()
                    .collect::<HashSet<String>>(),
            )
        };
        let snapshot_suffix = self.snapshot.as_ref().map(|tag| make_snapshot_suffix(tag));

        let plan = assemble_release_plan(
            &engine_projects,
            &intents,
            &ledger,
            Some(&config.versioning),
            &AssembleReleasePlanOptions { filter, snapshot_suffix: snapshot_suffix.clone() },
        )?;

        if plan.releases.is_empty() {
            println!(r#"No pending changes. Record one with "pnpm change"."#);
            return Ok(());
        }
        if self.dry_run {
            println!("{}", render_release_plan(&plan));
            return Ok(());
        }

        let applied = apply_release_plan(
            &plan,
            &workspace_dir,
            &intents,
            Some(&config.versioning),
            ApplyReleasePlanOptions { snapshot: snapshot_suffix.is_some() },
        )?;

        use std::fmt::Write as _;
        let mut output = String::from("Versions applied:\n");
        for release in &applied {
            writeln!(
                output,
                "{}: {} → {}",
                release.name, release.current_version, release.new_version,
            )
            .expect("write to string");
        }
        println!("{output}");
        Ok(())
    }

    fn handle_prerelease_line(&self, config: &Config, action: &str) -> miette::Result<()> {
        let Some(workspace_dir) = config.workspace_dir.clone() else {
            return Err(VersionError::PrereleaseLineOutsideWorkspace {
                action: action.to_string(),
            }
            .into());
        };
        if config.filter.is_empty() {
            let example =
                if action == "unstable" { "unstable alpha" } else { "stable" }.to_string();
            return Err(VersionError::FilterRequired { example }.into());
        }

        let (projects, _) = discover_workspace_projects(&workspace_dir)?;
        let engine_projects = to_engine_projects(&projects);
        let releasable: HashSet<String> =
            releasable_pkg_names(&engine_projects, &config.versioning).into_iter().collect();
        let selected: Vec<String> = selected_pkg_names(&projects, config, &workspace_dir)?
            .into_iter()
            .filter(|name| releasable.contains(name))
            .collect();
        if selected.is_empty() {
            return Err(VersionError::NoPackagesSelected.into());
        }

        let mut settings: VersioningSettings = config.versioning.clone();
        let selected_lines: String = selected.iter().fold(String::new(), |mut lines, name| {
            use std::fmt::Write as _;
            writeln!(lines, "  {name}").expect("write to string");
            lines
        });
        let output = if action == "unstable" {
            let tag = self.params.get(1).cloned().unwrap_or_default();
            // A purely numeric tag is rejected because semver parses an
            // all-digit prerelease identifier as a number, which changes
            // sorting semantics.
            let valid_tag = !tag.is_empty()
                && tag
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
                && !tag.chars().all(|character| character.is_ascii_digit());
            if !valid_tag {
                return Err(VersionError::InvalidPrereleaseTag.into());
            }
            for name in &selected {
                if let Some(existing) = settings.prereleases.get(name)
                    && existing != &tag
                {
                    return Err(VersionError::AlreadyOnLine {
                        pkg_name: name.clone(),
                        tag: existing.clone(),
                    }
                    .into());
                }
                settings.prereleases.insert(name.clone(), tag.clone());
            }
            format!("Entered the \"{tag}\" prerelease line:\n{selected_lines}")
        } else {
            for name in &selected {
                settings.prereleases.shift_remove(name);
            }
            format!(
                r#"Exited the prerelease line:
{selected_lines}The accumulated stable versions release on the next "pnpm version -r" run."#,
            )
        };

        let value = if settings.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::to_value(&settings).expect("versioning settings serialize to JSON")
        };
        update_manifest_field(&workspace_dir.join("pnpm-workspace.yaml"), "versioning", &value)
            .map_err(miette::Report::new)?;
        println!("{output}");
        Ok(())
    }
}

/// The names of the projects the active `--filter` selectors pick, in graph
/// order.
fn selected_pkg_names(
    projects: &[pacquet_workspace::Project],
    config: &Config,
    prefix: &Path,
) -> miette::Result<Vec<String>> {
    let selection = select_recursive_projects(projects, config, prefix, AutoExcludeRoot::Disabled)?;
    Ok(selection
        .selected
        .values()
        .filter_map(|node| {
            node.package.project.manifest.value().get("name").and_then(|name| name.as_str())
        })
        .map(ToString::to_string)
        .collect())
}

fn make_snapshot_suffix(tag: &str) -> String {
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    if tag.is_empty() { timestamp.to_string() } else { format!("{tag}-{timestamp}") }
}
