//! `pacquet outdated` — report direct dependencies that have a newer
//! version available.
//!
//! The detection half — [`collect_outdated`] — is shared with
//! `update --interactive`, which gathers the same "what has a newer
//! version" list before prompting. The two callers differ only in which
//! registry version counts as the comparison [`TargetVersion`]: `outdated`
//! compares against the absolute newest (`latest` tag, or the highest
//! in-range version under `--compatible`), while `update` compares against
//! the version a bump would move to.
//!
//! Scope vs. pnpm: pacquet loads the *wanted* lockfile, so there is no
//! separate *current* lockfile to diff against — a dependency's `current`
//! and `wanted` versions are always equal, and the "missing (wanted X)"
//! state pnpm shows for a resolved-but-not-installed dependency does not
//! arise.

pub use query::{OutdatedPackage, OutdatedQuery, TargetVersion, collect_outdated};
pub(crate) use query::{
    OutdatedRun, collect_outdated_for_importer, collect_outdated_for_importer_in_run,
    ignored_dependencies_matcher,
};
pub(crate) use render::colorize_target;

use crate::{
    State,
    cli_args::{
        catalogs::configured_catalogs,
        install::resolve_bool_override,
        recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
        sanitize::sanitize_inline,
    },
};
use clap::{Args, ValueEnum};
use miette::IntoDiagnostic;
use node_semver::Version;
use owo_colors::Stream;
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_resolver::{
    CatalogResolutionResult, WantedDependency as CatalogWantedDependency, resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_github_actions as github_actions;
use pnpm_lockfile::Lockfile;
use pnpm_matcher::{Matcher, create_matcher};
use pnpm_network::ThrottledClient;
use pnpm_package_manager::{PickPolicy, create_configured_npm_resolver};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use pnpm_resolving_npm_resolver::{InMemoryPackageMetaCache, NpmResolver};
use pnpm_resolving_resolver_base::{
    LatestQuery, ResolveOptions, WantedDependency as ResolverWantedDependency,
};

use render::{
    render_json, render_list, render_recursive_json, render_recursive_list, render_recursive_table,
    render_table, sort_outdated, sort_workspace_outdated, write_output,
};
use std::{borrow::Cow, collections::HashMap, io::Write, path::PathBuf, sync::Arc};
use workspace::{
    DependentProject, OutdatedInWorkspace, ProjectOutdatedInputs, isolated_global_config,
    loaded_lockfile, no_lockfile_error, project_dir, recursive_project_inputs, workspace_outdated,
};

/// Output format for `pacquet outdated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutdatedFormat {
    Table,
    List,
    Json,
}

/// `--prod` / `--dev` / `--no-optional` for `pacquet outdated`.
#[derive(Debug, Args)]
pub struct OutdatedDependencyOptions {
    /// Check only "dependencies" and "optionalDependencies".
    #[clap(short = 'P', long, visible_alias = "production")]
    prod: bool,
    /// Check only "devDependencies".
    #[clap(short = 'D', long)]
    dev: bool,
    /// Don't check "optionalDependencies".
    #[clap(long, overrides_with = "optional")]
    no_optional: bool,
    /// Include "optionalDependencies".
    #[clap(long, overrides_with = "no_optional")]
    optional: bool,
}

impl OutdatedDependencyOptions {
    fn include(&self, include_optional: bool) -> Vec<DependencyGroup> {
        let mut optional = resolve_bool_override(self.optional, self.no_optional, include_optional);
        let (production, dev) = if self.prod {
            (true, false)
        } else if self.dev {
            optional = false;
            (false, true)
        } else {
            (true, true)
        };
        std::iter::empty()
            .chain(production.then_some(DependencyGroup::Prod))
            .chain(dev.then_some(DependencyGroup::Dev))
            .chain(optional.then_some(DependencyGroup::Optional))
            .collect()
    }
}

/// `pacquet outdated [<pkg> ...]`.
#[derive(Debug, Args)]
pub struct OutdatedArgs {
    /// Restrict the check to dependencies whose name matches one of these
    /// patterns (`*` wildcard, leading `!` to negate). With no arguments,
    /// every direct dependency in the included groups is checked.
    pub packages: Vec<String>,

    /// --prod, --dev, and --no-optional.
    #[clap(flatten)]
    pub dependency_options: OutdatedDependencyOptions,

    /// Print only versions that satisfy the ranges in package.json.
    #[clap(long)]
    pub compatible: bool,

    /// Print details about the outdated packages (homepage, deprecation
    /// notice).
    #[clap(long)]
    pub long: bool,

    /// Output format.
    #[clap(long, value_enum, default_value_t = OutdatedFormat::Table)]
    pub format: OutdatedFormat,

    /// Shorthand for `--format list`. Good for small consoles.
    #[clap(long = "no-table")]
    pub no_table: bool,

    /// Shorthand for `--format json`.
    #[clap(long)]
    pub json: bool,

    /// Sorting method. Currently only `name` is supported; the default
    /// sorts by the size of the version change, then by name.
    #[clap(long, value_enum)]
    pub sort_by: Option<SortBy>,

    /// Also check GitHub Actions dependencies in workflow and action files.
    #[clap(long = "include-github-actions")]
    pub include_github_actions: bool,

    /// Check globally installed packages.
    #[clap(short = 'g', long)]
    pub global: bool,
}

/// Sort order for the outdated report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortBy {
    Name,
}

/// Whether `outdated` found any outdated dependency. The CLI harness maps
/// [`OutdatedOutcome::Outdated`] to a process exit code of `1`, matching
/// pnpm; returning the outcome (rather than terminating here) keeps
/// [`OutdatedArgs::run`] composable and process termination in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutdatedOutcome {
    UpToDate,
    Outdated,
}

impl OutdatedArgs {
    /// Run the check and print the report to stdout. Returns whether any
    /// dependency was outdated; the caller decides the process exit code.
    pub async fn run<Reporter: self::Reporter>(
        self,
        state: State,
    ) -> miette::Result<OutdatedOutcome> {
        if state.config.recursive {
            return self.run_recursive::<Reporter>(state).await;
        }

        let config = state.config;
        let manifest = &state.manifest;
        let root = config.workspace_dir.as_deref().unwrap_or_else(|| project_dir(manifest));
        let importer_id = state.active_importer_id();
        let lockfile = loaded_lockfile(&state)?;
        let package_patterns = self.package_patterns();
        let check_packages = self.checks_packages(manifest, &package_patterns);
        if check_packages && lockfile.is_none() {
            return Err(no_lockfile_error(project_dir(manifest)));
        }

        let filters = OutdatedFilters::new(&self, config, &package_patterns);
        let query = filters.query(self.target_version());
        let mut outdated = if check_packages {
            collect_outdated_for_importer(
                manifest,
                lockfile,
                &importer_id,
                config,
                &state.http_client,
                &query,
            )
            .await?
        } else {
            Vec::new()
        };
        outdated.extend(
            self.outdated_actions::<Reporter>(
                config,
                root,
                &filters.include,
                github_actions::selector_matcher(&self.packages).as_ref(),
            )
            .await?
            .into_iter()
            .map(OutdatedPackage::from),
        );

        self.report_outdated(&mut outdated)
    }

    fn report_outdated(&self, outdated: &mut [OutdatedPackage]) -> miette::Result<OutdatedOutcome> {
        sort_outdated(outdated, self.sort_by);
        self.write_rendered(outdated)?;

        Ok(if outdated.is_empty() { OutdatedOutcome::UpToDate } else { OutdatedOutcome::Outdated })
    }

    /// The `outdated <pattern>` arguments that select packages rather than
    /// workflow actions.
    fn package_patterns(&self) -> Vec<String> {
        self.packages
            .iter()
            .filter(|selector| !github_actions::is_selector(selector))
            .cloned()
            .collect()
    }

    fn write_rendered(&self, outdated: &[OutdatedPackage]) -> miette::Result<()> {
        let output = match self.resolve_format() {
            OutdatedFormat::Table => render_table(outdated, self.long),
            OutdatedFormat::List => render_list(outdated, self.long),
            OutdatedFormat::Json => render_json(outdated, self.long),
        };
        write_output(&output)
    }

    fn write_recursive_rendered(&self, outdated: &[OutdatedInWorkspace]) -> miette::Result<()> {
        let output = match self.resolve_format() {
            OutdatedFormat::Table => render_recursive_table(outdated, self.long),
            OutdatedFormat::List => render_recursive_list(outdated, self.long),
            OutdatedFormat::Json => render_recursive_json(outdated, self.long),
        };
        write_output(&output)
    }

    /// `--compatible` reports the newest version the declared range
    /// still admits; the default reports the newest published one.
    fn target_version(&self) -> TargetVersion {
        if self.compatible { TargetVersion::WithinRange } else { TargetVersion::Latest }
    }

    /// Whether the run inspects lockfile packages at all. An empty
    /// package manifest requires no lockfile, and neither does a run
    /// whose selectors name only workflow actions — but the workflows
    /// are still inspected.
    fn checks_packages(&self, manifest: &PackageManifest, package_patterns: &[String]) -> bool {
        let has_any_dependency = manifest
            .dependencies([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional])
            .next()
            .is_some();
        has_any_dependency && (self.packages.is_empty() || !package_patterns.is_empty())
    }

    /// The outdated GitHub Actions of the workflows under `root`. Empty
    /// unless the run includes dev dependencies and opted in to the
    /// check.
    async fn outdated_actions<Reporter: self::Reporter>(
        &self,
        config: &Config,
        root: &std::path::Path,
        include: &[DependencyGroup],
        action_matcher: Option<&Matcher>,
    ) -> miette::Result<Vec<github_actions::OutdatedGitHubAction>> {
        if !include.contains(&DependencyGroup::Dev)
            || !crate::github_actions::opted_in(self.include_github_actions, config)
        {
            return Ok(Vec::new());
        }
        github_actions::find_outdated::<Reporter>(
            root,
            self.compatible,
            action_matcher,
            config.update_config.github_actions_server.as_deref(),
        )
        .await
    }

    async fn run_recursive<Reporter: self::Reporter>(
        self,
        state: State,
    ) -> miette::Result<OutdatedOutcome> {
        let config = state.config;
        let workspace_root =
            config.workspace_dir.clone().unwrap_or_else(|| state.lockfile_dir().to_path_buf());
        let (projects, _) = discover_workspace_projects(&workspace_root, config)?;
        let selection = select_recursive_projects(
            &projects,
            config,
            project_dir(&state.manifest),
            AutoExcludeRoot::Disabled,
        )?;
        let filters = OutdatedFilters::new(&self, config, &self.packages);
        let query = filters.query(self.target_version());

        // Every project reads the one shared lockfile, or its own.
        let shared_lockfile =
            if config.shares_one_lockfile() { loaded_lockfile(&state)? } else { None };
        let project_inputs = recursive_project_inputs(config, &selection)?;
        let run = OutdatedRun::new(config, Arc::clone(&state.http_client))?;
        let mut outdated = workspace_outdated(
            &ProjectOutdatedInputs {
                config,
                lockfile_root: state.lockfile_dir(),
                shared_lockfile,
                query: &query,
                run: &run,
            },
            &project_inputs,
        )
        .await?;

        outdated.extend(
            self.workspace_outdated_actions::<Reporter>(config, &workspace_root, &filters.include)
                .await?,
        );

        sort_workspace_outdated(&mut outdated);
        self.write_recursive_rendered(&outdated)?;

        Ok(if outdated.is_empty() { OutdatedOutcome::UpToDate } else { OutdatedOutcome::Outdated })
    }

    async fn workspace_outdated_actions<Reporter: self::Reporter>(
        &self,
        config: &Config,
        workspace_root: &std::path::Path,
        include: &[DependencyGroup],
    ) -> miette::Result<Vec<OutdatedInWorkspace>> {
        let action_matcher = github_actions::selector_matcher(&self.packages);
        Ok(self
            .outdated_actions::<Reporter>(config, workspace_root, include, action_matcher.as_ref())
            .await?
            .into_iter()
            .map(|action| OutdatedInWorkspace {
                package: OutdatedPackage::from(action),
                dependents: vec![DependentProject {
                    name: ".github".to_string(),
                    location: workspace_root.to_path_buf(),
                }],
            })
            .collect())
    }

    /// `pnpm outdated -g`: inspect every globally installed package group,
    /// treating each install dir's `package.json` as a project, and report
    /// the aggregate.
    pub async fn run_global(self, config: &'static Config) -> miette::Result<OutdatedOutcome> {
        let global_pkg_dir = config.global_pkg_dir.clone().ok_or_else(|| {
            miette::miette!(
                code = "ERR_PNPM_NO_GLOBAL_BIN_DIR",
                "Unable to find the global packages directory"
            )
        })?;
        let config = isolated_global_config(config);
        let filters = OutdatedFilters::new(&self, config, &self.packages);
        let query = filters.query(self.target_version());

        let mut outdated = Vec::new();
        let global_packages = pnpm_global::scan_global_packages(&global_pkg_dir)
            .map_err(|err| miette::miette!("failed to scan global packages: {err}"))?;
        for pkg in global_packages {
            let state = State::init(pkg.install_dir.join("package.json"), config, false)
                .map_err(|err| miette::Report::new(err).wrap_err("initialize global state"))?;
            outdated.extend(
                collect_outdated(
                    &state.manifest,
                    loaded_lockfile(&state)?,
                    config,
                    &state.http_client,
                    &query,
                )
                .await?,
            );
        }

        sort_outdated(&mut outdated, self.sort_by);
        self.write_rendered(&outdated)?;

        Ok(if outdated.is_empty() { OutdatedOutcome::UpToDate } else { OutdatedOutcome::Outdated })
    }

    /// Collapse the `--format` flag and its `--no-table` / `--json`
    /// shorthands into one format. The shorthands win over an explicit
    /// `--format`, with `--json` taking precedence over `--no-table`,
    /// mirroring pnpm's shorthand expansion order.
    fn resolve_format(&self) -> OutdatedFormat {
        if self.json {
            OutdatedFormat::Json
        } else if self.no_table {
            OutdatedFormat::List
        } else {
            self.format
        }
    }
}

/// The dependency groups and the name matchers one outdated query walks.
struct OutdatedFilters {
    include: Vec<DependencyGroup>,
    match_names: Option<Matcher>,
    ignore_names: Option<Matcher>,
}

impl OutdatedFilters {
    fn new(args: &OutdatedArgs, config: &Config, package_patterns: &[String]) -> Self {
        Self {
            include: args.dependency_options.include(config.optional),
            match_names: (!package_patterns.is_empty()).then(|| create_matcher(package_patterns)),
            ignore_names: ignored_dependencies_matcher(config),
        }
    }

    fn query(&self, target_version: TargetVersion) -> OutdatedQuery<'_> {
        OutdatedQuery {
            target_version,
            include_direct: &self.include,
            match_names: self.match_names.as_ref(),
            ignore_names: self.ignore_names.as_ref(),
            include_deprecated: true,
        }
    }
}

#[cfg(test)]
mod tests;

mod render;

mod query;

mod workspace;
