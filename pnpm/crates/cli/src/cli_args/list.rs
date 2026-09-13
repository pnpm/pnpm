//! `pnpm list` / `ls` / `ll` / `la` — list installed packages.

pub(crate) mod render;

use crate::cli_args::{
    deps_tree::{
        build::{
            BuildTreeOptions, DependenciesHierarchy, LoadedState, build_dependencies_tree,
            importer_root_ids,
        },
        get_tree::MaxDepth,
        graph::{BuildGraphOptions, build_dependency_graph},
        search::Searcher,
    },
    deps_tree_finders::{evaluate_finders, finder_candidates, resolve_finders},
    install::resolve_bool_override,
    recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
};
use clap::Args;
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_global::{ListReportAs, find_global_install_dirs, list_global_packages};
use pnpm_modules_yaml::IncludedDependencies;
use render::{ProjectHierarchy, RenderParseableOptions, RenderTreeOptions};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RecursionLimit {
    ProjectsOnly,
    Levels(u32),
    Unlimited,
}

fn parse_depth(text: &str) -> Result<RecursionLimit, String> {
    if text.eq_ignore_ascii_case("Infinity") || text == "-1" {
        return Ok(if text == "-1" {
            RecursionLimit::ProjectsOnly
        } else {
            RecursionLimit::Unlimited
        });
    }
    let n: u32 = text
        .parse()
        .map_err(|_| format!("expected a non-negative integer, Infinity, or -1, got `{text}`"))?;
    Ok(RecursionLimit::Levels(n))
}

impl RecursionLimit {
    fn max_depth(self) -> MaxDepth {
        match self {
            RecursionLimit::ProjectsOnly => MaxDepth::Finite(0),
            RecursionLimit::Levels(levels) => MaxDepth::Finite(u64::from(levels)),
            RecursionLimit::Unlimited => MaxDepth::Unlimited,
        }
    }
}

#[derive(Debug, Args)]
pub struct ListArgs {
    pub packages: Vec<String>,
    #[clap(short = 'g', long)]
    pub global: bool,
    /// Exclude peer dependencies.
    #[clap(long)]
    pub exclude_peers: bool,
    /// Search by a finder function declared in `.pnpmfile.cjs`.
    #[clap(long = "find-by")]
    pub find_by: Vec<String>,
    #[clap(flatten)]
    pub output: TreeOutputArgs,
    #[clap(flatten)]
    pub dependencies: TreeDependencyArgs,
    #[clap(flatten)]
    pub graph: ListGraphArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct TreeOutputArgs {
    /// Show extended information.
    #[clap(long)]
    pub long: bool,
    /// Show information in JSON format.
    #[clap(long)]
    pub json: bool,
    /// Show parseable output instead of tree view.
    #[clap(long)]
    pub parseable: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct TreeDependencyArgs {
    /// Display only the dependency graph for packages in `dependencies`
    /// and `optionalDependencies`.
    #[clap(short = 'P', long = "prod", visible_alias = "production")]
    pub production: bool,
    /// Display only the dependency graph for packages in `devDependencies`.
    #[clap(short = 'D', long)]
    pub dev: bool,
    /// Don't display packages from `optionalDependencies`.
    #[clap(long, overrides_with = "optional")]
    pub no_optional: bool,
    /// Include packages from `optionalDependencies`.
    #[clap(long, overrides_with = "no_optional")]
    pub optional: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ListGraphArgs {
    /// Max display depth of the dependency tree. `0` lists direct
    /// dependencies only; `-1` lists projects only.
    #[clap(long, default_value = "0", value_parser = parse_depth, allow_hyphen_values = true)]
    pub(crate) depth: RecursionLimit,
    /// Display only dependencies that are also projects within the
    /// workspace.
    #[clap(long)]
    pub only_projects: bool,
    /// List packages from the lockfile only, without checking
    /// `node_modules`.
    #[clap(long)]
    pub lockfile_only: bool,
}

impl ListArgs {
    pub async fn run(self, config: &Config, dir: &Path, recursive: bool) -> miette::Result<()> {
        let output = if self.global {
            self.run_global(config).await?
        } else if recursive {
            self.run_recursive(config, dir).await?
        } else {
            let lockfile_dir = local_lockfile_dir(config, dir);
            self.render_projects(
                config,
                &[dir.to_path_buf()],
                &self.packages,
                &lockfile_dir,
                true,
            )
            .await?
        };
        print_output(&output);
        Ok(())
    }

    async fn run_global(&self, config: &Config) -> miette::Result<String> {
        let global_pkg_dir = config.global_pkg_dir
            .clone()
            .ok_or_else(|| {
                miette::miette!(
                    code = "ERR_PNPM_NO_GLOBAL_BIN_DIR",
                    "Unable to find the global packages directory"
                )
            })?;

        if (matches!(self.graph.depth, RecursionLimit::Levels(n) if n > 0)
            || self.graph.depth == RecursionLimit::Unlimited)
            && let Some(output) = self.render_global_tree(config, &global_pkg_dir)
                .await?
        {
            return Ok(output);
        }

        let report_as = self.report_as();
        list_global_packages(
            &global_pkg_dir,
            &self.packages,
            global_report_as(report_as),
            self.output.long,
        )
        .into_diagnostic()
    }

    async fn render_global_tree(
        &self,
        config: &Config,
        global_pkg_dir: &Path,
    ) -> miette::Result<Option<String>> {
        let all_install_dirs = find_global_install_dirs(global_pkg_dir, &[]).into_diagnostic()?;
        if all_install_dirs.len() == 1 {
            // Single global install: keep params so the search can
            // cover the whole tree, matching regular `pnpm ls`.
            let install_dir = all_install_dirs[0].clone();
            return self
                .render_projects(
                    config,
                    std::slice::from_ref(&install_dir),
                    &self.packages,
                    &install_dir,
                    true,
                )
                .await
                .map(Some);
        }
        // Multiple installs — try to narrow to a single one via
        // params, matching against top-level aliases of each
        // install group.
        let matching_install_dirs =
            find_global_install_dirs(global_pkg_dir, &self.packages).into_diagnostic()?;
        if matching_install_dirs.len() > 1
            || (matching_install_dirs.is_empty() && !all_install_dirs.is_empty())
        {
            return Err(miette::miette!(
                code = "ERR_PNPM_GLOBAL_LS_DEPTH_NOT_SUPPORTED",
                "Cannot list a merged dependency tree across multiple global packages. \
                     Each global package is installed in an isolated directory with its own lockfile, \
                     so transitive dependencies cannot be coherently merged. \
                     Filter to a single global package by its top-level name, or omit --depth."
            ));
        }
        if let [install_dir] = matching_install_dirs.as_slice() {
            // Params served their purpose of narrowing to a single
            // install group; passing them on would activate search
            // semantics, which prune the matched package's children.
            return self
                .render_projects(
                    config,
                    std::slice::from_ref(install_dir),
                    &[],
                    install_dir,
                    true,
                )
                .await
                .map(Some);
        }
        Ok(None)
    }

    async fn run_recursive(&self, config: &Config, dir: &Path) -> miette::Result<String> {
        let (workspace_root, project_dirs) = listed_project_dirs(config, dir)?;

        let always_print_root_package = self.graph.depth == RecursionLimit::ProjectsOnly;

        if config.shares_one_lockfile() {
            return self.render_projects(
                config,
                &project_dirs,
                &self.packages,
                config.lockfile_dir_for(&workspace_root),
                always_print_root_package,
            )
            .await;
        }

        // Per-project lockfiles: each project renders independently
        // (with its own legend and summary).
        let mut outputs = Vec::new();
        for project_dir in project_dirs {
            let output = self.render_projects(
                config,
                std::slice::from_ref(&project_dir),
                &self.packages,
                &project_dir,
                always_print_root_package,
            )
            .await?;
            if !output.is_empty() {
                outputs.push(output);
            }
        }
        let joiner = if self.graph.depth == RecursionLimit::ProjectsOnly {
            "\n"
        } else {
            "\n\n"
        };
        Ok(outputs.join(joiner))
    }

    fn report_as(&self) -> ReportAs {
        if self.output.parseable {
            ReportAs::Parseable
        } else if self.output.json {
            ReportAs::Json
        } else {
            ReportAs::Tree
        }
    }

    fn include(&self, include_optional: bool) -> IncludedDependencies {
        let has_both = self.dependencies.production == self.dependencies.dev;
        IncludedDependencies {
            dependencies: has_both || self.dependencies.production,
            dev_dependencies: has_both || self.dependencies.dev,
            optional_dependencies: resolve_bool_override(
                self.dependencies.optional,
                self.dependencies.no_optional,
                include_optional,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportAs {
    Tree,
    Json,
    Parseable,
}

fn global_report_as(report_as: ReportAs) -> ListReportAs {
    match report_as {
        ReportAs::Tree => ListReportAs::Tree,
        ReportAs::Json => ListReportAs::Json,
        ReportAs::Parseable => ListReportAs::Parseable,
    }
}

/// The directory the lockfile is read from for a non-recursive `list`.
pub(crate) fn local_lockfile_dir(config: &Config, dir: &Path) -> PathBuf {
    config.lockfile_dir_for(dir).to_path_buf()
}

/// Print command output the way the TypeScript CLI does: nothing for an
/// empty string, exactly one trailing newline otherwise.
pub(crate) fn print_output(output: &str) {
    if output.is_empty() {
        return;
    }
    if output.ends_with('\n') {
        print!("{output}");
    } else {
        println!("{output}");
    }
}

#[cfg(test)]
mod tests;

fn listed_project_dirs(config: &Config, dir: &Path) -> miette::Result<(PathBuf, Vec<PathBuf>)> {
    let workspace_root = config.workspace_dir
        .clone()
        .unwrap_or_else(|| dir.to_path_buf());
    let (projects, _) = discover_workspace_projects(&workspace_root, config)?;
    let selection = select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
    let project_dirs: Vec<PathBuf> = selection.selected
        .keys()
        .cloned()
        .collect();
    Ok((workspace_root, project_dirs))
}

fn project_hierarchy(
    (project_dir, hierarchy): (PathBuf, DependenciesHierarchy),
) -> ProjectHierarchy {
    let manifest = crate::cli_args::deps_tree::build::read_project_manifest(&project_dir);
    ProjectHierarchy {
        name: manifest.name,
        version: manifest.version,
        private: manifest.private,
        path: project_dir.to_string_lossy().into_owned(),
        hierarchy,
    }
}

mod listing;
