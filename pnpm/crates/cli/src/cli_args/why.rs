//! `pnpm why` — show the packages that depend on `<pkg>`.

use std::{collections::HashMap, path::PathBuf};

use clap::Args;
use pnpm_modules_yaml::IncludedDependencies;

use crate::{
    State,
    cli_args::{
        deps_tree::{
            build::{LoadedState, importer_root_ids, read_project_manifest, safe_importer_dir},
            dependents::{BuildDependentsOptions, ImporterInfo, build_dependents_tree},
            dependents_render::{
                RenderDependentsOptions, render_dependents_json, render_dependents_parseable,
                render_dependents_tree,
            },
            graph::{BuildGraphOptions, build_dependency_graph},
            search::Searcher,
        },
        deps_tree_finders::{evaluate_finders, finder_candidates, resolve_finders},
        install::resolve_bool_override,
        list::print_output,
        recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
    },
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

    /// Exclude peer dependencies.
    ///
    /// Accepted but not applied, matching the TypeScript CLI: its `why`
    /// command declares the flag without forwarding it to the
    /// dependents-tree builder.
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
        let lockfile_dir = state.lockfile_dir().to_path_buf();
        let project_dir = state
            .manifest
            .path()
            .parent()
            .expect("manifest path always has a parent dir")
            .to_path_buf();

        let project_dirs: Vec<PathBuf> = if state.config.recursive {
            let workspace_root = state.config.workspace_dir.as_deref().unwrap_or(&lockfile_dir);
            let (projects, _) = discover_workspace_projects(workspace_root, state.config)?;
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
        let Some(env) = loaded.env(
            &lockfile_dir,
            state.config.virtual_store_dir_max_length as usize,
            &state.config.resolved_registries(),
            state.config.registry_options_by_url.clone(),
        ) else {
            return Ok(());
        };
        let lockfile = env.current_lockfile;

        let mut importer_info: HashMap<String, ImporterInfo> = HashMap::new();
        for importer_id in lockfile.importers.keys() {
            // A key that cannot be safely joined (a malformed or
            // hostile lockfile) is never dereferenced; the raw key
            // still names the importer in the output.
            let manifest = safe_importer_dir(&lockfile_dir, importer_id)
                .map(|importer_dir| read_project_manifest(&importer_dir))
                .unwrap_or_default();
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
                optional_dependencies: resolve_bool_override(
                    self.optional,
                    self.no_optional,
                    state.config.optional,
                ),
            }
        };

        let root_ids = importer_root_ids(lockfile, &lockfile_dir, &project_dirs);
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
            manifest_fields: &[],
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
