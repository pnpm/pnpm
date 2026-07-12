use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_publish::{Host, is_git_repo, is_working_tree_clean};
use pacquet_versioning::{
    ApplyReleasePlanOptions, AssembleReleasePlanOptions, apply_release_plan, assemble_release_plan,
    read_change_intents, read_ledger,
};
use std::{collections::HashSet, path::Path};

use crate::cli_args::{
    change::{render_release_plan, to_engine_projects},
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
};

/// `pnpm version` — apply the pending change intents (`-r` with no version
/// argument). The npm-style explicit-bump forms are not ported yet;
/// per-package release lanes are managed by `pnpm lane`.
#[derive(Debug, Args)]
pub struct VersionArgs {
    /// An npm-style version argument (not ported yet); the bare recursive
    /// form consumes the pending change intents instead.
    pub params: Vec<String>,

    /// Print the release plan the pending change intents produce without
    /// applying it.
    #[clap(long = "dry-run")]
    pub dry_run: bool,

    /// Release one-off snapshot versions (0.0.0-<timestamp>, or
    /// 0.0.0-<tag>-<timestamp> when a tag is given) without consuming change
    /// intents. Intended for CI preview publishing: publish, then discard
    /// the manifest changes.
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

    #[display("Working tree is not clean. Commit or stash your changes.")]
    #[diagnostic(code(ERR_PNPM_UNCLEAN_WORKING_TREE))]
    UncleanWorkingTree,

    #[display(
        "Invalid snapshot tag: {tag}. The tag must form a valid semver prerelease identifier."
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_SNAPSHOT_TAG))]
    InvalidSnapshotTag { tag: String },
}

impl VersionArgs {
    pub fn run(self, config: &Config, recursive: bool) -> miette::Result<()> {
        match self.params.first().map(String::as_str) {
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
                selected_projects(&projects, config, &workspace_dir)?
                    .into_iter()
                    .map(|(_, dir)| dir)
                    .collect::<HashSet<String>>(),
            )
        };
        let snapshot_suffix =
            self.snapshot.as_ref().map(|tag| make_snapshot_suffix(tag)).transpose()?;

        let plan = assemble_release_plan(
            &engine_projects,
            &workspace_dir,
            &intents,
            &ledger,
            Some(&config.versioning),
            &AssembleReleasePlanOptions { filter, snapshot_suffix: snapshot_suffix.clone() },
        )?;

        if plan.releases.is_empty() {
            // Even an empty plan can leave garbage-collectable intent files
            // behind: declined ("none"-only) intents and files a merge
            // resurrected after every named package had already consumed
            // them.
            if !self.dry_run && snapshot_suffix.is_none() {
                apply_release_plan(
                    &plan,
                    &workspace_dir,
                    &engine_projects,
                    &intents,
                    Some(&config.versioning),
                    ApplyReleasePlanOptions::default(),
                )?;
            }
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
            &engine_projects,
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
}

/// The projects the active `--filter` selectors pick, in graph order, as
/// `(name, workspace-relative dir)` pairs.
pub(crate) fn selected_projects(
    projects: &[pacquet_workspace::Project],
    config: &Config,
    workspace_dir: &Path,
) -> miette::Result<Vec<(Option<String>, String)>> {
    let selection =
        select_recursive_projects(projects, config, workspace_dir, AutoExcludeRoot::Disabled)?;
    Ok(selection
        .selected
        .iter()
        .map(|(root_dir, node)| {
            let name = node
                .package
                .project
                .manifest
                .value()
                .get("name")
                .and_then(|name| name.as_str())
                .map(ToString::to_string);
            (name, pacquet_versioning::to_project_dir(workspace_dir, root_dir))
        })
        .collect())
}

fn make_snapshot_suffix(tag: &str) -> miette::Result<String> {
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let suffix = if tag.is_empty() { timestamp.to_string() } else { format!("{tag}-{timestamp}") };
    if node_semver::Version::parse(format!("0.0.0-{suffix}")).is_err() {
        return Err(VersionError::InvalidSnapshotTag { tag: tag.to_string() }.into());
    }
    Ok(suffix)
}
