use super::{ListArgs, RecursionLimit, ReportAs, render};
use crate::cli_args::recursive::{AutoExcludeRoot, RecursiveSelection, select_recursive_projects};
use pnpm_config::Config;
use pnpm_workspace_projects_graph::BaseProject;
use std::path::{Path, PathBuf};

impl ListArgs {
    pub(super) async fn run_recursive(
        &self,
        config: &Config,
        dir: &Path,
    ) -> miette::Result<String> {
        let workspace_root = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        let projects = self.discover_listed_projects(&workspace_root, config)?;
        let selection =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
        let project_dirs = ordered_project_dirs(&selection, config);

        let always_print_root_package = self.always_print_selected_projects();

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

        if self.report_as() == ReportAs::Json {
            return self.render_recursive_json(config, &selection, &project_dirs).await;
        }

        // Per-project lockfiles: each project renders independently
        // (with its own legend and summary).
        let mut outputs = Vec::new();
        for project_dir in &project_dirs {
            let project = &selection.selected[project_dir];
            let project_config =
                dedicated_project_config(config, project_dir, project.package.manifest_name());
            let output = self.render_projects(
                &project_config,
                std::slice::from_ref(project_dir),
                &self.packages,
                project_dir,
                always_print_root_package,
            )
            .await?;
            if !output.is_empty() {
                outputs.push(output);
            }
        }
        let joiner = if self.graph.depth == RecursionLimit::ProjectsOnly { "\n" } else { "\n\n" };
        Ok(outputs.join(joiner))
    }

    /// Whether every selected project is printed, including one with
    /// nothing listed under it. `--depth -1` lists only the projects, and
    /// so does `--only-projects` unless a search narrows it to the
    /// projects it matched in.
    fn always_print_selected_projects(&self) -> bool {
        self.graph.depth == RecursionLimit::ProjectsOnly
            || (self.graph.only_projects
                && self.packages.is_empty()
                && self.find_by.is_empty())
    }

    /// Every selected project's hierarchy in one JSON array. Joining the
    /// arrays the projects render on their own would not parse.
    async fn render_recursive_json(
        &self,
        config: &Config,
        selection: &RecursiveSelection<'_>,
        project_dirs: &[PathBuf],
    ) -> miette::Result<String> {
        let mut projects = Vec::new();
        for project_dir in project_dirs {
            let project = &selection.selected[project_dir];
            let project_config =
                dedicated_project_config(config, project_dir, project.package.manifest_name());
            projects.extend(
                self.load_project_hierarchies(
                    &project_config,
                    std::slice::from_ref(project_dir),
                    &self.packages,
                    project_dir,
                )
                .await?,
            );
        }
        Ok(render::render_json(&projects, self.output.long))
    }
}

fn ordered_project_dirs(selection: &RecursiveSelection<'_>, config: &Config) -> Vec<PathBuf> {
    let mut project_dirs = if config.sort {
        selection.sequenced_dirs()
    } else {
        selection.selected
            .keys()
            .cloned()
            .collect()
    };
    if config.reverse {
        project_dirs.reverse();
    }
    project_dirs
}

/// `config` re-anchored on one project of a workspace whose projects keep
/// their own lockfiles, so the listing reads the modules directory that
/// project installed into rather than the workspace-wide one.
pub(super) fn dedicated_project_config(
    config: &Config,
    project_dir: &Path,
    project_name: Option<&str>,
) -> Config {
    let mut project_config = config.clone();
    project_config.anchor_dedicated_project(project_dir, project_name);
    project_config
}
