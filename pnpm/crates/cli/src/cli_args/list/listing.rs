use super::{
    BuildGraphOptions, BuildTreeOptions, Config, DependenciesHierarchy, ListArgs, LoadedState,
    Path, PathBuf, ProjectHierarchy, RecursionLimit, RenderParseableOptions, RenderTreeOptions,
    ReportAs, Searcher, build_dependencies_tree, build_dependency_graph, evaluate_finders,
    finder_candidates, importer_root_ids, project_hierarchy, render, resolve_finders,
};

impl ListArgs {
    pub(super) async fn render_projects(
        &self,
        config: &Config,
        project_dirs: &[PathBuf],
        params: &[String],
        lockfile_dir: &Path,
        always_print_root_package: bool,
    ) -> miette::Result<String> {
        let state = LoadedState::load(
            lockfile_dir,
            Some(config.modules_dir.as_path()),
            self.graph.lockfile_only,
        )?;
        let env = state.env(
            lockfile_dir,
            config.virtual_store_dir_max_length as usize,
            &config.resolved_registries(),
            config.registry_options_by_url.clone(),
        );

        let hierarchies = match env
            .as_ref()
            .filter(|_| self.graph.depth != RecursionLimit::ProjectsOnly)
        {
            Some(env) => {
                self.build_hierarchies(config, &state, env, project_dirs, lockfile_dir, params)
                    .await?
            }
            // Without a materialized `node_modules` there is no tree to
            // walk; every project reports its own line and nothing under
            // it.
            None => project_dirs
                .iter()
                .map(|project_dir| (project_dir.clone(), DependenciesHierarchy::default()))
                .collect(),
        };

        let projects: Vec<ProjectHierarchy> = hierarchies
            .into_iter()
            .map(project_hierarchy)
            .collect();

        self.render_project_hierarchies(&projects, always_print_root_package)
    }

    pub(super) fn render_project_hierarchies(
        &self,
        projects: &[ProjectHierarchy],
        always_print_root_package: bool,
    ) -> miette::Result<String> {
        Ok(match self.report_as() {
            ReportAs::Tree => render::render_tree(
                projects,
                &RenderTreeOptions {
                    always_print_root_package,
                    depth_above_projects_only: self.graph.depth != RecursionLimit::ProjectsOnly,
                    long: self.output.long,
                    show_extraneous: false,
                    show_summary: true,
                },
            ),
            ReportAs::Parseable => render::render_parseable(
                projects,
                &RenderParseableOptions {
                    long: self.output.long,
                    always_print_root_package,
                },
            ),
            ReportAs::Json => render::render_json(projects, self.output.long),
        })
    }

    /// Walk the dependency graph of every listed project, applying the
    /// search queries and `--find-by` finders when the command has any.
    pub(super) async fn build_hierarchies(
        &self,
        config: &Config,
        state: &LoadedState,
        env: &pnpm_deps_inspection::pkg_info::PkgInfoEnv<'_>,
        project_dirs: &[PathBuf],
        lockfile_dir: &Path,
        params: &[String],
    ) -> miette::Result<Vec<(PathBuf, DependenciesHierarchy)>> {
        let include = self.include(config.optional);
        let root_ids = importer_root_ids(env.current_lockfile, lockfile_dir, project_dirs);
        let graph = build_dependency_graph(
            &root_ids,
            &BuildGraphOptions {
                lockfile: env.current_lockfile,
                include,
                only_projects: self.graph.only_projects,
            },
        );
        let searcher = self.build_searcher(config, env, &graph, lockfile_dir, params)
            .await?;
        build_dependencies_tree(
            state,
            env,
            &graph,
            project_dirs,
            &BuildTreeOptions {
                lockfile_dir,
                depth: self.graph.depth.max_depth(),
                include,
                exclude_peer_dependencies: self.exclude_peers,
                only_projects: self.graph.only_projects,
                search: searcher.as_ref(),
                show_deduped_search_matches: searcher.is_some(),
                modules_dir_opt: Some(config.modules_dir.as_path()),
            },
        )
    }

    /// The searcher the tree walk filters through. `None` when the
    /// command named no query and no finder.
    pub(super) async fn build_searcher(
        &self,
        config: &Config,
        env: &pnpm_deps_inspection::pkg_info::PkgInfoEnv<'_>,
        graph: &pnpm_deps_inspection::graph::DependencyGraph,
        lockfile_dir: &Path,
        params: &[String],
    ) -> miette::Result<Option<Searcher>> {
        if params.is_empty() && self.find_by.is_empty() {
            return Ok(None);
        }
        let mut searcher = Searcher::from_queries(params)?;
        if !self.find_by.is_empty() {
            let finders = resolve_finders(config, lockfile_dir, &self.find_by)
                .await?;
            let candidates = finder_candidates(env, graph);
            let results = evaluate_finders(env, &finders, candidates).await?;
            searcher.set_finder_results(results);
        }
        Ok(Some(searcher))
    }
}
