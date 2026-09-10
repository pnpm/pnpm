use super::{
    Arc, AutoExcludeRoot, Catalogs, Config, HashMap, Mutex, PackArgs, PackError, PackOutputLocks,
    PackResultJson, Path, PathBuf, PnpmfileHooks, Reporter, absolute_against, configured_catalogs,
    discover_workspace_projects, filtered_projects_dependencies, format_pack_output,
    graph_sequencer, pack_output_path, select_recursive_projects,
};

/// The shared inputs of every project's pack in a recursive run, and the
/// results the packs report into.
pub(super) struct RecursivePack<'a, 'graph> {
    pub(super) config: &'a Config,
    pub(super) graph:
        &'a pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'graph>>,
    pub(super) catalogs: Catalogs,
    pub(super) out: Option<String>,
    pub(super) pack_destination: Option<String>,
    pub(super) before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    pub(super) output_locks: Arc<PackOutputLocks>,
    /// Each project's position in dependency order, which the results are
    /// listed in.
    pub(super) order_index: HashMap<PathBuf, usize>,
    pub(super) packed: Mutex<Vec<(usize, PackResultJson)>>,
    pub(super) first_error: Mutex<Option<miette::Report>>,
}

fn dependency_order(
    project_dependencies: &indexmap::IndexMap<PathBuf, Vec<PathBuf>>,
) -> Vec<PathBuf> {
    graph_sequencer(
        &project_dependencies
            .iter()
            .map(|(project, dependencies)| (project.clone(), dependencies.clone()))
            .collect::<HashMap<_, _>>(),
        &project_dependencies.keys().cloned().collect::<Vec<_>>(),
    )
    .order
}

/// Whether `--out` names one file for every project, with no `%s` / `%v`
/// placeholders telling them apart.
fn output_is_literal(out: Option<&str>) -> bool {
    out.is_some_and(|out| !out.contains("%s") && !out.contains("%v"))
}

pub(super) fn order_index(dependency_order: Vec<PathBuf>) -> HashMap<PathBuf, usize> {
    dependency_order.into_iter().enumerate().map(|(index, project)| (project, index)).collect()
}

/// Order two projects that pack to the same output path against each
/// other, so the later one overwrites the earlier deterministically
/// rather than racing it.
fn serialize_shared_outputs(
    project_dependencies: &mut indexmap::IndexMap<PathBuf, Vec<PathBuf>>,
    graph: &pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'_>>,
    dependency_order: &[PathBuf],
    out: Option<&str>,
    pack_destination: Option<&str>,
) {
    let mut previous_by_output = HashMap::<PathBuf, PathBuf>::new();
    for root in dependency_order {
        let project = graph[root].package.project;
        let manifest = project.manifest.value();
        let Some(name) = manifest.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(version) = manifest.get("version").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let published_name = manifest
            .pointer("/publishConfig/name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(name);
        let predecessor =
            pack_output_path(&project.root_dir, out, pack_destination, published_name, version)
                .ok()
                .and_then(|output| previous_by_output.insert(output, root.clone()));
        if let Some(predecessor) = predecessor {
            let dependencies = project_dependencies
                .get_mut(root)
                .expect("ordered project exists in dependency graph");
            if !dependencies.contains(&predecessor) {
                dependencies.push(predecessor);
            }
        }
    }
}

/// Whether a project's tarball path can change while the run packs, so
/// two projects that would otherwise share an output must be ordered
/// against each other rather than checked up front. A `prepack` or
/// `prepare` script and a `publishConfig.directory` both rewrite the
/// manifest the output name comes from.
fn output_can_change_while_packing(
    config: &Config,
    graph: &pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'_>>,
    before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
) -> bool {
    if !before_packing_hooks.is_empty() {
        return true;
    }
    let runs_pack_scripts = !config.ignore_scripts
        && graph.values().any(|node| {
            let manifest = node.package.project.manifest.value();
            ["prepack", "prepare"].iter().any(|script| {
                manifest
                    .pointer(&format!("/scripts/{script}"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|body| !body.is_empty())
            })
        });
    runs_pack_scripts
        || graph.values().any(|node| {
            node.package.project.manifest.value().pointer("/publishConfig/directory").is_some()
        })
}

fn render_recursive_pack(
    packed: &[PackResultJson],
    dir: &Path,
    json: bool,
) -> miette::Result<String> {
    if packed.is_empty() {
        tracing::info!(
            target: "pacquet::pack",
            prefix = %dir.display(),
            "There are no packages that should be packed",
        );
        return Ok(String::new());
    }
    Ok(format_pack_output(packed, json, false))
}

impl PackArgs {
    pub(super) async fn run_recursive<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    ) -> miette::Result<String> {
        self.validate_recursive_destination()?;
        // `pack` is not in pnpm's root-auto-exclusion command set, so the
        // workspace root stays in the selection (its own name/version
        // eligibility check still applies below).
        let (projects, _) =
            discover_workspace_projects(config.workspace_dir.as_deref().unwrap_or(dir), config)?;
        let selection =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
        let graph = &selection.selected;
        let mut project_dependencies = filtered_projects_dependencies(
            graph,
            selection.full_graph(),
            selection.prod_all.as_ref(),
            &selection.prod_only_selected,
        );

        // In recursive mode `--out` / `--pack-destination` resolves to an
        // absolute path against the CLI dir (and defaults the destination
        // to the CLI dir), so every tarball lands in one place regardless
        // of each project's own root.
        let (out, pack_destination) = self.resolve_recursive_destination(dir);
        let dependency_order = dependency_order(&project_dependencies);
        if !output_can_change_while_packing(config, graph, &before_packing_hooks)
            || output_is_literal(out.as_deref())
        {
            serialize_shared_outputs(
                &mut project_dependencies,
                graph,
                &dependency_order,
                out.as_deref(),
                pack_destination.as_deref(),
            );
        }

        let pack = RecursivePack {
            config,
            graph,
            catalogs: configured_catalogs(config)?,
            out,
            pack_destination,
            before_packing_hooks,
            output_locks: Arc::new(PackOutputLocks::default()),
            order_index: order_index(dependency_order),
            packed: Mutex::new(Vec::new()),
            first_error: Mutex::new(None),
        };
        pack.execute::<Reporter>(self, &project_dependencies).await;
        let packed = pack.finish()?;

        render_recursive_pack(&packed, dir, self.json)
    }

    fn validate_recursive_destination(&self) -> miette::Result<()> {
        if self.out.is_some() && self.pack_destination.is_some() {
            return Err(miette::Report::new(PackError::OutAndPackDestination));
        }
        Ok(())
    }

    /// Resolve the recursive-mode `(out, pack_destination)` pair to
    /// absolute paths against the CLI `dir`.
    fn resolve_recursive_destination(&self, dir: &Path) -> (Option<String>, Option<String>) {
        if let Some(out) = &self.out {
            (Some(absolute_against(dir, out)), None)
        } else if let Some(destination) = &self.pack_destination {
            (None, Some(absolute_against(dir, destination)))
        } else {
            (None, Some(dir.to_string_lossy().into_owned()))
        }
    }
}
