//! `pacquet pack` — create a tarball from a package.
//!
//! The single-project work lives in [`pnpm_pack::api`]; this module
//! maps the resolved [`Config`] and CLI flags onto
//! [`pnpm_pack::PackOptions`], and drives the recursive (`-r`) sweep
//! over the workspace the same way the other recursive commands do.
//!
//! Recursive packing dispatches dependency-ready projects up to the
//! configured workspace concurrency.

use crate::cli_args::{
    catalogs::configured_catalogs,
    install::resolve_bool_override,
    recursive::{
        AutoExcludeRoot, discover_workspace_projects, filtered_projects_dependencies,
        select_recursive_projects,
    },
};
use clap::Args;
use miette::Context;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_pack::{
    Host, PackError, PackOptions, PackOutputLocks, PackResultJson, api, format_pack_output,
    pack_output_path, to_pack_result_json,
};
use pnpm_reporter::{LifecycleMessage, LifecycleStdio, LogEvent, Reporter};
use pnpm_workspace_task_scheduler::{
    ScheduleGraphAsyncOptions, TaskCompletion, graph_sequencer, schedule_graph_async,
};
use std::{
    collections::HashMap,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// The `wrap_err` framing `pack` and `publish` attach to a failed pack.
/// [`super::dispatch`]'s `--json` error path matches on it to surface the
/// underlying pack diagnostic instead of this wrapper, so the two sites must
/// share one definition.
pub(crate) const PACK_ERROR_CONTEXT: &str = "pack the package";

/// The reporter for `pack --json`. It stands in for the selected
/// `--reporter` / `--loglevel` (which pnpm skips entirely under `--json`)
/// and mirrors pnpm running the pack lifecycle scripts with inherited stdio:
/// the `$ <script>` banner goes to stderr, each script line stays on the
/// stream it was written to, and every other event is dropped so the JSON
/// result (or JSON error) is the last thing on stdout.
pub(super) struct PackJsonReporter;

impl Reporter for PackJsonReporter {
    fn emit(event: &LogEvent) {
        let LogEvent::Lifecycle(log) = event else {
            return;
        };
        match &log.message {
            LifecycleMessage::Script { script, .. } => {
                let _ = writeln!(io::stderr().lock(), "$ {script}");
            }
            LifecycleMessage::Stdio { line, stdio, .. } => match stdio {
                LifecycleStdio::Stdout => {
                    let _ = writeln!(io::stdout().lock(), "{line}");
                }
                LifecycleStdio::Stderr => {
                    let _ = writeln!(io::stderr().lock(), "{line}");
                }
            },
            LifecycleMessage::Exit { .. } => {}
        }
    }
}

/// Create a tarball from a package.
#[derive(Debug, Args)]
pub struct PackArgs {
    /// Do everything `pack` would do except writing the tarball to disk.
    #[clap(long)]
    pub dry_run: bool,

    /// Directory in which to save the tarball. Defaults to the current
    /// working directory.
    #[clap(long = "pack-destination")]
    pub pack_destination: Option<String>,

    /// Print the packed tarball and its contents in JSON.
    #[clap(long)]
    pub json: bool,

    /// Customize the output path. `%s` expands to the package name and
    /// `%v` to the version, e.g. `%s.tgz` or `some-dir/%s-%v.tgz`.
    #[clap(long)]
    pub out: Option<String>,

    /// gzip compression level (`0`–`9`) for the tarball.
    #[clap(long = "pack-gzip-level", value_parser = clap::value_parser!(u32).range(0..=9))]
    pub pack_gzip_level: Option<u32>,

    /// Keep the original `packageManager` field and publish-lifecycle
    /// scripts in the packed manifest instead of stripping them.
    #[clap(long = "skip-manifest-obfuscation", overrides_with = "no_skip_manifest_obfuscation")]
    pub skip_manifest_obfuscation: bool,
    /// Apply pnpm's normal packed-manifest filtering.
    #[clap(
        long = "no-skip-manifest-obfuscation",
        hide = true,
        overrides_with = "skip_manifest_obfuscation"
    )]
    pub no_skip_manifest_obfuscation: bool,
}

impl PackArgs {
    /// Pack the project at `dir` (or the `--filter`-selected workspace
    /// projects when `recursive`), returning the text/JSON the CLI prints.
    pub async fn run<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        recursive: bool,
        before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    ) -> miette::Result<String> {
        if recursive {
            self.run_recursive::<Reporter>(dir, config, before_packing_hooks).await
        } else {
            let mut options = self.pack_options(
                dir.to_path_buf(),
                config,
                configured_catalogs(config)?,
                self.out.clone(),
                self.pack_destination.clone(),
                before_packing_hooks,
            );
            set_injected_changelog(&mut options, config, dir).await?;
            let result = api::<Reporter, Host>(&options)
                .await
                .map_err(miette::Report::new)
                .wrap_err(PACK_ERROR_CONTEXT)?;
            Ok(format_pack_output(&[to_pack_result_json(&result)], self.json, false))
        }
    }

    async fn run_recursive<Reporter: self::Reporter>(
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

    async fn pack_one<Reporter: self::Reporter>(
        &self,
        config: &Config,
        project: &pnpm_workspace::Project,
        mut options: PackOptions,
    ) -> miette::Result<pnpm_pack::PackResult> {
        set_injected_changelog(&mut options, config, &project.root_dir).await?;
        api::<Reporter, Host>(&options).await.map_err(miette::Report::new).wrap_err_with(|| {
            if self.json {
                PACK_ERROR_CONTEXT.to_string()
            } else {
                format!("pack {}", project.root_dir.display())
            }
        })
    }

    /// Pack each `--filter`-selected workspace project that declares both
    /// a name and a version, in topological order.
    /// The pack options for one project, or `None` when the project has
    /// no name or version to pack under.
    fn packable_options(
        &self,
        project: &pnpm_workspace::Project,
        config: &Config,
        catalogs: Catalogs,
        out: Option<String>,
        pack_destination: Option<String>,
        before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    ) -> Option<PackOptions> {
        let manifest = project.manifest.value();
        let declares = |field: &str| {
            manifest
                .get(field)
                .and_then(|value| value.as_str())
                .is_some_and(|value| !value.is_empty())
        };
        if !declares("name") || !declares("version") {
            return None;
        }
        Some(self.pack_options(
            project.root_dir.clone(),
            config,
            catalogs,
            out,
            pack_destination,
            before_packing_hooks,
        ))
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

    /// Map `self` plus the resolved `config` onto a [`PackOptions`].
    ///
    /// `before_packing_hooks` is loaded once by the caller and cloned in
    /// (like `catalogs`) so a recursive pack shares one worker per
    /// pnpmfile across every project.
    fn pack_options(
        &self,
        dir: PathBuf,
        config: &Config,
        catalogs: Catalogs,
        out: Option<String>,
        pack_destination: Option<String>,
        before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    ) -> PackOptions {
        PackOptions {
            dir,
            catalogs,
            ignore_scripts: config.ignore_scripts,
            unsafe_perm: config.unsafe_perm,
            embed_readme: config.embed_readme,
            pack_gzip_level: self.pack_gzip_level,
            node_linker: config.node_linker,
            skip_manifest_obfuscation: resolve_bool_override(
                self.skip_manifest_obfuscation,
                self.no_skip_manifest_obfuscation,
                config.skip_manifest_obfuscation,
            ),
            user_agent: config.user_agent.clone(),
            extra_bin_paths: config.extra_bin_paths.clone(),
            extra_env: config.extra_env.clone(),
            workspace_dir: config.workspace_dir.clone(),
            dry_run: self.dry_run,
            out,
            pack_destination,
            before_packing_hooks,
            injected_files: Vec::new(),
            output_locks: None,
        }
    }
}

/// The shared inputs of every project's pack in a recursive run, and the
/// results the packs report into.
struct RecursivePack<'a, 'graph> {
    config: &'a Config,
    graph: &'a pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'graph>>,
    catalogs: Catalogs,
    out: Option<String>,
    pack_destination: Option<String>,
    before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    output_locks: Arc<PackOutputLocks>,
    /// Each project's position in dependency order, which the results are
    /// listed in.
    order_index: HashMap<PathBuf, usize>,
    packed: Mutex<Vec<(usize, PackResultJson)>>,
    first_error: Mutex<Option<miette::Report>>,
}

impl RecursivePack<'_, '_> {
    async fn execute<Reporter: self::Reporter>(
        &self,
        args: &PackArgs,
        project_dependencies: &indexmap::IndexMap<PathBuf, Vec<PathBuf>>,
    ) {
        let run_node = |root: PathBuf| self.pack_node::<Reporter>(args, root);
        schedule_graph_async(
            project_dependencies,
            &ScheduleGraphAsyncOptions::new(
                usize::try_from(self.config.workspace_concurrency).unwrap_or(usize::MAX).max(1),
                true,
                &run_node,
                &|_: &PathBuf| {},
            ),
        )
        .await;
    }

    async fn pack_node<Reporter: self::Reporter>(
        &self,
        args: &PackArgs,
        root: PathBuf,
    ) -> TaskCompletion {
        let project = self.graph[&root].package.project;
        let Some(mut options) = args.packable_options(
            project,
            self.config,
            self.catalogs.clone(),
            self.out.clone(),
            self.pack_destination.clone(),
            self.before_packing_hooks.clone(),
        ) else {
            return TaskCompletion::Passed;
        };
        options.output_locks = Some(Arc::clone(&self.output_locks));
        match args.pack_one::<Reporter>(self.config, project, options).await {
            Ok(result) => {
                self.packed
                    .lock()
                    .expect("packed results lock is not poisoned")
                    .push((self.order_index[&root], to_pack_result_json(&result)));
                TaskCompletion::Passed
            }
            Err(error) => {
                self.first_error
                    .lock()
                    .expect("pack error lock is not poisoned")
                    .get_or_insert(error);
                TaskCompletion::Failed
            }
        }
    }

    /// The results in dependency order, or the first pack error.
    fn finish(self) -> miette::Result<Vec<PackResultJson>> {
        if let Some(error) = self.first_error.into_inner().expect("pack error lock is not poisoned")
        {
            return Err(error);
        }
        let mut packed = self.packed.into_inner().expect("packed results lock is not poisoned");
        packed.sort_unstable_by_key(|(index, _)| *index);
        Ok(packed.into_iter().map(|(_, result)| result).collect())
    }
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

fn order_index(dependency_order: Vec<PathBuf>) -> HashMap<PathBuf, usize> {
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

/// Composes and injects the `registry`-storage CHANGELOG.md for the project at
/// `project_dir`, replacing any composed entry already set. A no-op in
/// `repository` storage or when the project has no parked section.
pub(crate) async fn set_injected_changelog(
    options: &mut PackOptions,
    config: &Config,
    project_dir: &Path,
) -> miette::Result<()> {
    if let Some(changelog) =
        crate::cli_args::changelog::compose_registry_changelog(config, project_dir).await?
    {
        options.injected_files = vec![("package/CHANGELOG.md".to_string(), changelog)];
    }
    Ok(())
}

/// Resolve `path` against `base` when it is relative, mirroring node's
/// `path.resolve(base, path)`.
fn absolute_against(base: &Path, path: &str) -> String {
    let path = if Path::new(path).is_absolute() { PathBuf::from(path) } else { base.join(path) };
    pnpm_fs::lexical_normalize(&path).to_string_lossy().into_owned()
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
