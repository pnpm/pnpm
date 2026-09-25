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
    Host, PackError, PackOptions, PackOutputLocks, PackResultJson, WorkspacePackageManifest, api,
    format_pack_output, pack_output_path, to_pack_result_json,
};
use pnpm_reporter::{LifecycleMessage, LifecycleStdio, LogEvent, Reporter};
use pnpm_workspace_task_scheduler::{
    ScheduleGraphAsyncOptions, TaskCompletion, graph_sequencer, schedule_graph_async,
};
use recursive::RecursivePack;
use std::{
    collections::HashMap,
    io,
    io::Write,
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

struct PackSharedContext {
    catalogs: Catalogs,
    out: Option<String>,
    pack_destination: Option<String>,
    before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    workspace_packages: Option<Arc<HashMap<String, WorkspacePackageManifest>>>,
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
            let shared = PackSharedContext {
                catalogs: configured_catalogs(config)?,
                out: self.out.clone(),
                pack_destination: self.pack_destination.clone(),
                before_packing_hooks,
                workspace_packages: None,
            };
            let mut options = self.pack_options(dir.to_path_buf(), config, shared);
            set_injected_changelog(&mut options, config, dir).await?;
            let result = api::<Reporter, Host>(&options).await
                .map_err(miette::Report::new)
                .wrap_err(PACK_ERROR_CONTEXT)?;
            Ok(format_pack_output(&[to_pack_result_json(&result)], self.json, false))
        }
    }

    async fn pack_one<Reporter: self::Reporter>(
        &self,
        config: &Config,
        project: &pnpm_workspace::Project,
        mut options: PackOptions,
    ) -> miette::Result<pnpm_pack::PackResult> {
        set_injected_changelog(&mut options, config, &project.root_dir).await?;
        api::<Reporter, Host>(&options).await
            .map_err(miette::Report::new)
            .wrap_err_with(|| {
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
        shared: PackSharedContext,
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
        Some(self.pack_options(project.root_dir.clone(), config, shared))
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
        shared: PackSharedContext,
    ) -> PackOptions {
        let workspace_packages = shared.workspace_packages.or_else(|| {
            crate::cli_args::workspace_packages::discover_workspace_package_manifests(
                config.workspace_dir.as_deref(),
                config,
            )
        });
        PackOptions {
            dir,
            workspace_dir: config.workspace_dir.clone(),
            scripts: pnpm_pack::PackScripts {
                ignore: config.ignore_scripts,
                unsafe_perm: config.unsafe_perm,
                user_agent: config.user_agent.clone(),
                extra_bin_paths: config.extra_bin_paths.clone(),
                extra_env: config.extra_env.clone(),
            },
            manifest: pnpm_pack::PackManifestOptions {
                catalogs: shared.catalogs,
                catalogs_dir: config.workspace_dir.clone(),
                embed_readme: config.embed_readme,
                node_linker: config.node_linker,
                skip_obfuscation: resolve_bool_override(
                    self.skip_manifest_obfuscation,
                    self.no_skip_manifest_obfuscation,
                    config.skip_manifest_obfuscation,
                ),
                format: config.preferred_manifest_format,
                before_packing_hooks: shared.before_packing_hooks,
                workspace_packages,
            },
            output: pnpm_pack::PackOutputOptions {
                gzip_level: self.pack_gzip_level,
                dry_run: self.dry_run,
                out: shared.out,
                destination: shared.pack_destination,
                injected_files: Vec::new(),
                locks: None,
            },
        }
    }
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
        let shared = PackSharedContext {
            catalogs: self.catalogs.clone(),
            out: self.output.out.clone(),
            pack_destination: self.output.destination.clone(),
            before_packing_hooks: self.before_packing_hooks.clone(),
            workspace_packages: self.workspace_packages.clone(),
        };
        let Some(mut options) = args.packable_options(project, self.config, shared) else {
            return TaskCompletion::Passed;
        };
        options.output.locks = Some(Arc::clone(&self.output.locks));
        match args.pack_one::<Reporter>(self.config, project, options).await {
            Ok(result) => {
                self.results.packed
                    .lock()
                    .expect("packed results lock is not poisoned")
                    .push((self.results.order_index[&root], to_pack_result_json(&result)));
                TaskCompletion::Passed
            }
            Err(error) => {
                self.results.first_error
                    .lock()
                    .expect("pack error lock is not poisoned")
                    .get_or_insert(error);
                TaskCompletion::Failed
            }
        }
    }

    /// The results in dependency order, or the first pack error.
    fn finish(self) -> miette::Result<Vec<PackResultJson>> {
        if let Some(error) =
            self.results.first_error.into_inner().expect("pack error lock is not poisoned")
        {
            return Err(error);
        }
        let mut packed =
            self.results.packed.into_inner().expect("packed results lock is not poisoned");
        packed.sort_unstable_by_key(|(index, _)| *index);
        Ok(packed
            .into_iter()
            .map(|(_, result)| result)
            .collect())
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
        options.output.injected_files = vec![("package/CHANGELOG.md".to_string(), changelog)];
    }
    Ok(())
}

/// Resolve `path` against `base` when it is relative, mirroring node's
/// `path.resolve(base, path)`.
fn absolute_against(base: &Path, path: &str) -> String {
    let path = if Path::new(path).is_absolute() { PathBuf::from(path) } else { base.join(path) };
    pnpm_fs::lexical_normalize(&path).to_string_lossy().into_owned()
}

mod recursive;
