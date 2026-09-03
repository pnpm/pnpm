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
use pnpm_reporter::Reporter;
use pnpm_workspace_task_scheduler::{
    ScheduleGraphAsyncOptions, TaskCompletion, graph_sequencer, schedule_graph_async,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// The `wrap_err` framing `pack` and `publish` attach to a failed pack.
/// [`super::dispatch`]'s `--json` error path matches on it to surface the
/// underlying pack diagnostic instead of this wrapper, so the two sites must
/// share one definition.
pub(crate) const PACK_ERROR_CONTEXT: &str = "pack the package";

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

    /// Pack each `--filter`-selected workspace project that declares both
    /// a name and a version, in topological order.
    async fn run_recursive<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    ) -> miette::Result<String> {
        // `--out` and `--pack-destination` are mutually exclusive. The
        // single-project path enforces this inside `api`; the recursive
        // path resolves a shared destination before `api` ever sees both,
        // so check here too rather than silently dropping one.
        if self.out.is_some() && self.pack_destination.is_some() {
            return Err(miette::Report::new(PackError::OutAndPackDestination));
        }
        let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
        // `pack` is not in pnpm's root-auto-exclusion command set, so the
        // workspace root stays in the selection (its own name/version
        // eligibility check still applies below).
        let (projects, _patterns) = discover_workspace_projects(workspace_root, config)?;
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
        let catalogs = configured_catalogs(config)?;
        let output_can_change_while_packing = !before_packing_hooks.is_empty()
            || (!config.ignore_scripts
                && graph.values().any(|node| {
                    let manifest = node.package.project.manifest.value();
                    ["prepack", "prepare"].iter().any(|script| {
                        manifest
                            .pointer(&format!("/scripts/{script}"))
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|body| !body.is_empty())
                    })
                }))
            || graph.values().any(|node| {
                node.package.project.manifest.value().pointer("/publishConfig/directory").is_some()
            });
        let dependency_order = graph_sequencer(
            &project_dependencies
                .iter()
                .map(|(project, dependencies)| (project.clone(), dependencies.clone()))
                .collect::<HashMap<_, _>>(),
            &project_dependencies.keys().cloned().collect::<Vec<_>>(),
        )
        .order;
        let output_is_literal =
            out.as_ref().is_some_and(|out| !out.contains("%s") && !out.contains("%v"));
        if !output_can_change_while_packing || output_is_literal {
            let mut previous_by_output = HashMap::<PathBuf, PathBuf>::new();
            for root in &dependency_order {
                let project = graph[root].package.project;
                let manifest = project.manifest.value();
                let Some(name) = manifest.get("name").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let Some(version) = manifest.get("version").and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                let published_name = manifest
                    .pointer("/publishConfig/name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(name);
                let predecessor = pack_output_path(
                    &project.root_dir,
                    out.as_deref(),
                    pack_destination.as_deref(),
                    published_name,
                    version,
                )
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
        let order_index: HashMap<PathBuf, usize> = dependency_order
            .into_iter()
            .enumerate()
            .map(|(index, project)| (project, index))
            .collect();

        let packed: Mutex<Vec<(usize, PackResultJson)>> = Mutex::new(Vec::new());
        let first_error: Mutex<Option<miette::Report>> = Mutex::new(None);
        let output_locks = Arc::new(PackOutputLocks::default());
        let run_node = |root: PathBuf| {
            let project_order = order_index[&root];
            let catalogs = catalogs.clone();
            let out = out.clone();
            let pack_destination = pack_destination.clone();
            let before_packing_hooks = before_packing_hooks.clone();
            let output_locks = Arc::clone(&output_locks);
            let packed = &packed;
            let first_error = &first_error;
            async move {
                let project = graph[&root].package.project;
                let manifest = project.manifest.value();
                let has_name = manifest
                    .get("name")
                    .and_then(|name| name.as_str())
                    .is_some_and(|name| !name.is_empty());
                let has_version = manifest
                    .get("version")
                    .and_then(|version| version.as_str())
                    .is_some_and(|version| !version.is_empty());
                if !has_name || !has_version {
                    return TaskCompletion::Passed;
                }
                let mut options = self.pack_options(
                    project.root_dir.clone(),
                    config,
                    catalogs.clone(),
                    out.clone(),
                    pack_destination.clone(),
                    before_packing_hooks.clone(),
                );
                options.output_locks = Some(output_locks);
                let result = async {
                    set_injected_changelog(&mut options, config, &project.root_dir).await?;
                    api::<Reporter, Host>(&options)
                        .await
                        .map_err(miette::Report::new)
                        .wrap_err_with(|| format!("pack {}", project.root_dir.display()))
                }
                .await;
                match result {
                    Ok(result) => {
                        packed
                            .lock()
                            .expect("packed results lock is not poisoned")
                            .push((project_order, to_pack_result_json(&result)));
                        TaskCompletion::Passed
                    }
                    Err(error) => {
                        first_error
                            .lock()
                            .expect("pack error lock is not poisoned")
                            .get_or_insert(error);
                        TaskCompletion::Failed
                    }
                }
            }
        };
        let on_node_skipped: fn(&PathBuf) = |_| {};
        schedule_graph_async(
            &project_dependencies,
            &ScheduleGraphAsyncOptions::new(
                usize::try_from(config.workspace_concurrency).unwrap_or(usize::MAX).max(1),
                true,
                &run_node,
                &on_node_skipped,
            ),
        )
        .await;
        if let Some(error) = first_error.into_inner().expect("pack error lock is not poisoned") {
            return Err(error);
        }
        let mut packed = packed.into_inner().expect("packed results lock is not poisoned");
        packed.sort_unstable_by_key(|(index, _)| *index);
        let packed = packed.into_iter().map(|(_, result)| result).collect::<Vec<_>>();

        if packed.is_empty() {
            tracing::info!(
                target: "pacquet::pack",
                prefix = %dir.display(),
                "There are no packages that should be packed",
            );
            return Ok(String::new());
        }
        Ok(format_pack_output(&packed, self.json, false))
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
