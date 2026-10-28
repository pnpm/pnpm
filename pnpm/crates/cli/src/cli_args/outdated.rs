//! `pacquet outdated` — report direct dependencies that have a newer
//! version available.

pub(crate) mod query;

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
        ecosystem_deps::{
            EcosystemFilterKind, declared_ecosystem_dep_names, patterns_for_ecosystem,
            requires_ecosystem_lockfile,
        },
        install::resolve_bool_override,
        recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
        sanitize::sanitize_inline,
    },
};
use clap::{Args, ValueEnum};
use filters::OutdatedFilters;
use miette::IntoDiagnostic;
use node_semver::Version;
use owo_colors::Stream;
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionResult, WantedDependency as CatalogWantedDependency,
    resolve_from_catalog,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_config_parse_overrides::parse_overrides_iter;
use pnpm_github_actions as github_actions;
use pnpm_lockfile::Lockfile;
use pnpm_matcher::{Matcher, create_matcher};
use pnpm_network::ThrottledClient;
use pnpm_package_manager::{PickPolicy, VersionsOverrider, create_configured_registry_resolver};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use pnpm_resolving_default_resolver::DefaultResolver;
use pnpm_resolving_resolver_base::{
    LatestQuery, ResolveOptions, WantedDependency as ResolverWantedDependency,
};
use render::{
    render_json, render_list, render_recursive_json, render_recursive_list, render_recursive_table,
    render_table, sort_outdated, sort_workspace_outdated, write_output,
};
use std::{borrow::Cow, collections::HashMap, io::Write, path::PathBuf, sync::Arc};
use workspace::{OutdatedInWorkspace, loaded_lockfile, no_lockfile_error, project_dir};

/// Output format for `pnpm outdated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutdatedFormat {
    Table,
    List,
    Json,
}

/// `--prod` / `--dev` / `--no-optional` for `pnpm outdated`.
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

/// `pnpm outdated [<pkg> ...]`.
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
    /// Also check GitHub Actions dependencies in workflow and action files.
    #[clap(long = "include-github-actions")]
    pub include_github_actions: bool,
    /// Check globally installed packages.
    #[clap(short = 'g', long)]
    pub global: bool,
    #[clap(flatten)]
    pub output: OutdatedOutputArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct OutdatedOutputArgs {
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
}

/// Sort order for the outdated report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortBy {
    Name,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutdatedOutcome {
    UpToDate,
    Outdated,
}

impl OutdatedArgs {
    pub async fn run<Reporter: self::Reporter>(
        self,
        state: State,
    ) -> miette::Result<OutdatedOutcome> {
        if state.config.recursive {
            return Box::pin(self.run_recursive::<Reporter>(state)).await;
        }
        let config = state.config;
        let manifest = &state.manifest;
        let package_patterns = self.package_patterns();
        let filters =
            OutdatedFilters::validated(&self, config, &package_patterns, [manifest], false)?;
        let lockfile = loaded_lockfile(&state)?;
        if self.requires_lockfile(manifest, config, &filters, &package_patterns, lockfile)? {
            return Err(no_lockfile_error(project_dir(manifest)));
        }
        let mut outdated = Box::pin(self.collect_single_project_outdated(
            &state,
            &filters,
            package_patterns,
            lockfile,
        ))
        .await?;
        let root = config.workspace_dir.as_deref().unwrap_or_else(|| project_dir(manifest));
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

    async fn collect_single_project_outdated(
        &self,
        state: &State,
        filters: &OutdatedFilters,
        package_patterns: Vec<String>,
        lockfile: Option<&Lockfile>,
    ) -> miette::Result<Vec<OutdatedPackage>> {
        if !self.checks_packages(
            &state.manifest,
            state.config,
            &filters.include,
            &package_patterns,
        )? {
            return Ok(Vec::new());
        }
        let query = filters.query(self.target_version());
        let run = OutdatedRun::new(state.config, Arc::clone(&state.http_client), &query)?
            .with_raw_packages(package_patterns);
        collect_outdated_for_importer_in_run(
            &state.manifest,
            lockfile,
            &state.active_importer_id(),
            &query,
            &run,
        )
        .await
    }

    fn report_outdated(&self, outdated: &mut [OutdatedPackage]) -> miette::Result<OutdatedOutcome> {
        sort_outdated(outdated, self.output.sort_by);
        self.write_rendered(outdated)?;

        Ok(if outdated.is_empty() { OutdatedOutcome::UpToDate } else { OutdatedOutcome::Outdated })
    }

    pub(super) fn package_patterns(&self) -> Vec<String> {
        self.packages
            .iter()
            .filter(|selector| !github_actions::is_selector(selector))
            .cloned()
            .collect()
    }

    pub(super) fn write_rendered(&self, outdated: &[OutdatedPackage]) -> miette::Result<()> {
        let output = match self.resolve_format() {
            OutdatedFormat::Table => render_table(outdated, self.output.long),
            OutdatedFormat::List => render_list(outdated, self.output.long),
            OutdatedFormat::Json => render_json(outdated, self.output.long),
        };
        write_output(&output)
    }

    fn write_recursive_rendered(&self, outdated: &[OutdatedInWorkspace]) -> miette::Result<()> {
        let output = match self.resolve_format() {
            OutdatedFormat::Table => render_recursive_table(outdated, self.output.long),
            OutdatedFormat::List => render_recursive_list(outdated, self.output.long),
            OutdatedFormat::Json => render_recursive_json(outdated, self.output.long),
        };
        write_output(&output)
    }

    pub(super) fn target_version(&self) -> TargetVersion {
        if self.compatible { TargetVersion::WithinRange } else { TargetVersion::Latest }
    }

    fn checks_packages(
        &self,
        manifest: &PackageManifest,
        config: &Config,
        include: &[DependencyGroup],
        package_patterns: &[String],
    ) -> miette::Result<bool> {
        let has_any_dependency = manifest
            .dependencies(include.iter().copied())
            .next()
            .is_some()
            || !crate::cli_args::engine_deps::declared_engine_deps(manifest, config, include)
                .is_empty()
            || !declared_ecosystem_dep_names(project_dir(manifest), config)?.is_empty();
        Ok(has_any_dependency && (self.packages.is_empty() || !package_patterns.is_empty()))
    }

    fn requires_lockfile(
        &self,
        manifest: &PackageManifest,
        config: &Config,
        filters: &OutdatedFilters,
        package_patterns: &[String],
        lockfile: Option<&Lockfile>,
    ) -> miette::Result<bool> {
        if !self.packages.is_empty() && package_patterns.is_empty() {
            return Ok(false);
        }
        let node_patterns = patterns_for_ecosystem(package_patterns, EcosystemFilterKind::Node)?;
        let checks_node = package_patterns.is_empty() || !node_patterns.is_empty();
        let query = filters.query(self.target_version());
        let needs_node_lockfile = checks_node
            && lockfile.is_none()
            && manifest
                .dependencies(filters.include.iter().copied())
                .any(|(name, spec)| {
                    !pnpm_tool_protocol::is_tool_specifier(spec) && query.matches_name(name)
                });
        let needs_eco_lockfile =
            requires_ecosystem_lockfile(project_dir(manifest), config, &query, package_patterns)?;
        Ok(needs_node_lockfile || needs_eco_lockfile)
    }

    pub(super) async fn outdated_actions<Reporter: self::Reporter>(
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
            crate::github_actions::release_age(config)?.as_ref(),
        )
        .await
    }

    fn resolve_format(&self) -> OutdatedFormat {
        if self.output.json {
            OutdatedFormat::Json
        } else if self.output.no_table {
            OutdatedFormat::List
        } else {
            self.output.format
        }
    }
}

mod filters;
#[cfg(test)]
mod tests;

mod render;

mod workspace;
