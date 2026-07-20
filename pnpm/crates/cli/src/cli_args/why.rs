//! `pnpm why` — show the packages that depend on `<pkg>`.

use std::{collections::HashMap, path::PathBuf};

use clap::Args;
use pacquet_modules_yaml::IncludedDependencies;

use crate::{
    State,
    cli_args::{
        deps_tree::{
            TreeNodeId,
            build::{LoadedState, importer_id_for, read_project_manifest},
            dependents::{BuildDependentsOptions, ImporterInfo, build_dependents_tree},
            finders::{evaluate_finders, finder_candidates, resolve_finders},
            graph::{BuildGraphOptions, build_dependency_graph},
            search::Searcher,
        },
        list::print_output,
        recursive::{
            AutoExcludeRoot, RecursiveSharedLockfileUnsupported, discover_workspace_projects,
            select_recursive_projects,
        },
    },
};

mod render;

use render::{
    RenderDependentsOptions, render_dependents_json, render_dependents_parseable,
    render_dependents_tree,
};

#[derive(Debug, Args)]
pub struct WhyArgs {
    pub packages: Vec<String>,

    /// Max display depth of the reverse dependency tree.
    #[clap(long)]
    pub depth: Option<usize>,

    /// Show extended information.
    #[clap(long)]
    pub long: bool,

    /// Show information in JSON format.
    #[clap(long)]
    pub json: bool,

    /// Show parseable output instead of tree view.
    #[clap(long)]
    pub parseable: bool,

    /// Display only the dependency graph for packages in `dependencies`
    /// and `optionalDependencies`.
    #[clap(short = 'P', long = "prod")]
    pub production: bool,

    /// Display only the dependency graph for packages in `devDependencies`.
    #[clap(short = 'D', long)]
    pub dev: bool,

    /// Don't display packages from `optionalDependencies`.
    #[clap(long)]
    pub no_optional: bool,

    /// Exclude peer dependencies.
    #[clap(long)]
    pub exclude_peers: bool,

    /// Search by a finder function declared in `.pnpmfile.cjs`.
    #[clap(long = "find-by")]
    pub find_by: Vec<String>,
}

impl WhyArgs {
    pub async fn run(self, state: State) -> miette::Result<()> {
        if self.packages.is_empty() && self.find_by.is_empty() {
            return Err(miette::miette!(
                code = "ERR_PNPM_MISSING_PACKAGE_NAME",
                "`pnpm why` requires the package name or --find-by=<finder-name>"
            ));
        }
        if state.config.recursive && !state.config.shared_workspace_lockfile {
            return Err(RecursiveSharedLockfileUnsupported::new(
                "Recursive and filtered `pnpm why`",
            )
            .into());
        }

        let lockfile_dir = state.lockfile_dir().to_path_buf();
        let project_dir = state
            .manifest
            .path()
            .parent()
            .expect("manifest path always has a parent dir")
            .to_path_buf();

        let project_dirs: Vec<PathBuf> = if state.config.recursive {
            let (projects, _) = discover_workspace_projects(&lockfile_dir)?;
            select_recursive_projects(
                &projects,
                state.config,
                &project_dir,
                AutoExcludeRoot::Disabled,
            )?
            .selected
            .keys()
            .cloned()
            .collect()
        } else {
            vec![project_dir]
        };

        let loaded =
            LoadedState::load(&lockfile_dir, Some(state.config.modules_dir.as_path()), false)?;
        let Some(env) =
            loaded.env(&lockfile_dir, state.config.virtual_store_dir_max_length as usize)
        else {
            return Ok(());
        };
        let lockfile = env.current_lockfile;

        let mut importer_info: HashMap<String, ImporterInfo> = HashMap::new();
        for importer_id in lockfile.importers.keys() {
            let manifest = read_project_manifest(&lockfile_dir.join(importer_id.as_str()));
            let name = manifest.name.unwrap_or_else(|| {
                if importer_id == "." {
                    "the root project".to_string()
                } else {
                    importer_id.clone()
                }
            });
            importer_info.insert(
                importer_id.clone(),
                ImporterInfo { name, version: manifest.version.unwrap_or_default() },
            );
        }

        let include = {
            let has_both = self.production == self.dev;
            IncludedDependencies {
                dependencies: has_both || self.production,
                dev_dependencies: has_both || self.dev,
                optional_dependencies: !self.no_optional,
            }
        };

        let root_ids: Vec<TreeNodeId> = project_dirs
            .iter()
            .map(|dir| TreeNodeId::Importer(importer_id_for(&lockfile_dir, dir)))
            .filter(|id| match id {
                TreeNodeId::Importer(importer_id) => {
                    lockfile.importers.contains_key(importer_id.as_str())
                }
                TreeNodeId::Package(_) => false,
            })
            .collect();
        let graph = build_dependency_graph(
            &root_ids,
            &BuildGraphOptions { lockfile, include, only_projects: false },
        );

        let mut searcher = Searcher::from_queries(&self.packages)?;
        if !self.find_by.is_empty() {
            let finders = resolve_finders(state.config, &lockfile_dir, &self.find_by).await?;
            let candidates = finder_candidates(&env, &graph);
            let results = evaluate_finders(&env, &finders, candidates).await?;
            searcher.set_finder_results(results);
        }

        let trees = build_dependents_tree(&BuildDependentsOptions {
            env: &env,
            graph: &graph,
            search: &searcher,
            importer_info: &importer_info,
        });

        let render_opts = RenderDependentsOptions { long: self.long, depth: self.depth };
        let output = if self.parseable {
            render_dependents_parseable(&trees, &render_opts)
        } else if self.json {
            render_dependents_json(&trees, &render_opts)
        } else {
            render_dependents_tree(&trees, &render_opts)
        };
        print_output(&output);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
