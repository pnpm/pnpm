//! `pacquet publish --recursive` — publish every selected workspace package.
//!
//! Selects the workspace projects the `--filter` selectors pick, drops the
//! private / unnamed / already-published ones (unless `--force`), then
//! publishes the rest in dependency order, optionally
//! writing `pnpm-publish-summary.json`.

use super::PublishArgs;
use crate::cli_args::{
    changelog::published_name,
    recursive::{
        AutoExcludeRoot, discover_workspace_projects, filtered_projects_dependencies,
        select_recursive_projects,
    },
    registry_client::build_registry_client,
};
use miette::{Context, IntoDiagnostic};
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_hooks::PnpmfileHooks;
use pnpm_network::{RetryOpts, ThrottledClient};
use pnpm_publish::{
    Host, PublishNetwork, PublishSummary, batch_publish_packed_pkgs, find_registry_info,
    resolve_otp_from_env, validate_batch_publish_options,
};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_npm_resolver::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, fetch_full_metadata,
};
use pnpm_workspace_task_scheduler::{
    ScheduleGraphAsyncOptions, TaskCompletion, graph_sequencer, schedule_graph_async,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

impl PublishArgs {
    /// Pack every project in dependency order, then publish the archives
    /// as one batch.
    async fn publish_batch<Reporter: self::Reporter>(
        &self,
        config: &Config,
        opts: &pnpm_publish::PublishPackedPkgOptions,
        network: &PublishNetwork<'_>,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        to_publish: &HashSet<PathBuf>,
        project_dependencies: &indexmap::IndexMap<PathBuf, Vec<PathBuf>>,
    ) -> miette::Result<Vec<PublishSummary>> {
        let mut packed = Vec::with_capacity(to_publish.len());
        let edges = project_dependencies
            .iter()
            .map(|(root, dependencies)| (root.clone(), dependencies.clone()))
            .collect();
        let order =
            graph_sequencer(&edges, &project_dependencies.keys().cloned().collect::<Vec<_>>())
                .order;
        for root in order.into_iter().filter(|root| to_publish.contains(root)) {
            packed
                .push(self.pack_directory::<Reporter>(&root, config, before_packing_hooks).await?);
        }
        let packages = packed.iter().map(|package| package.packed_pkg()).collect::<Vec<_>>();
        batch_publish_packed_pkgs::<Reporter, _, miette::Report>(
            &packages,
            opts,
            network,
            |package_indexes| {
                for &package_index in package_indexes {
                    self.run_post_publish_scripts::<Reporter>(&packed[package_index], config)?;
                }
                Ok(())
            },
        )
        .await
    }

    async fn publish_selected<Reporter: self::Reporter>(
        &self,
        config: &Config,
        opts: &pnpm_publish::PublishPackedPkgOptions,
        network: &PublishNetwork<'_>,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        to_publish: &HashSet<PathBuf>,
        project_dependencies: &indexmap::IndexMap<PathBuf, Vec<PathBuf>>,
    ) -> miette::Result<Vec<PublishSummary>> {
        if self.flags.batch {
            self.publish_batch::<Reporter>(
                config,
                opts,
                network,
                before_packing_hooks,
                to_publish,
                project_dependencies,
            )
            .await
        } else {
            self.publish_one_by_one::<Reporter>(
                config,
                opts,
                network,
                before_packing_hooks,
                to_publish,
                project_dependencies,
            )
            .await
        }
    }

    fn checked_recursive_publish_options(
        &self,
        config: &Config,
        stage: bool,
    ) -> miette::Result<pnpm_publish::PublishPackedPkgOptions> {
        let opts = self.publish_options(
            config,
            resolve_otp_from_env::<Host>(self.flags.otp.clone()),
            stage,
        );
        if self.flags.batch {
            validate_batch_publish_options(&opts)?;
        }

        Ok(opts)
    }

    pub(super) async fn run_recursive<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        stage: bool,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
    ) -> miette::Result<Vec<PublishSummary>> {
        let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
        // `publish` is not in pnpm's root-auto-exclusion command set
        // (`run` / `exec` / `add` / `test`), so the workspace root stays in the
        // selection; its own name/version/private eligibility check drops it
        // below.
        let (projects, _patterns) = discover_workspace_projects(workspace_root, config)?;
        let selection =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
        let graph = &selection.selected;
        // An empty selection is a no-op (exit 0) that writes no summary —
        // whether the workspace enumerates no project at all or a `--filter`
        // narrowed it to nothing: publishing returns before the handler when
        // there are no projects at all or the selection is empty.
        if graph.is_empty() {
            return Ok(Vec::new());
        }

        let http_client = build_registry_client(config)?;
        let network = PublishNetwork { client: &http_client, auth_headers: &config.auth_headers };
        let opts = self.checked_recursive_publish_options(config, stage)?;

        // Filter the selected graph: keep only packages that have a name and
        // version, are not private, and — unless `--force` — are not already on
        // their registry. The already-published probes are independent registry
        // reads, so run them concurrently rather than one round-trip at a time
        // (the `ThrottledClient` still bounds the actual in-flight fan-out).
        let to_publish = self
            .projects_to_publish(graph, config, &http_client, retry_opts_from_config(config))
            .await;

        if to_publish.is_empty() {
            emit_info::<Reporter>("There are no new packages that should be published", dir);
            self.write_summary(workspace_root, &[])?;
            return Ok(Vec::new());
        }

        let project_dependencies = filtered_projects_dependencies(
            graph,
            selection.full_graph(),
            selection.prod_all.as_ref(),
            &selection.prod_only_selected,
        );
        let published = self
            .publish_selected::<Reporter>(
                config,
                &opts,
                &network,
                before_packing_hooks,
                &to_publish,
                &project_dependencies,
            )
            .await?;
        self.write_summary(workspace_root, &published)?;
        Ok(published)
    }

    /// Publish in dependency order, one project at a time: an OTP challenge
    /// is interactive and per-process.
    async fn publish_one_by_one<Reporter: self::Reporter>(
        &self,
        config: &Config,
        opts: &pnpm_publish::PublishPackedPkgOptions,
        network: &PublishNetwork<'_>,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        to_publish: &HashSet<PathBuf>,
        project_dependencies: &indexmap::IndexMap<PathBuf, Vec<PathBuf>>,
    ) -> miette::Result<Vec<PublishSummary>> {
        let published: Mutex<Vec<PublishSummary>> = Mutex::new(Vec::new());
        let first_error: Mutex<Option<miette::Report>> = Mutex::new(None);
        let run_node = |root: PathBuf| {
            let published = &published;
            let first_error = &first_error;
            async move {
                if !to_publish.contains(&root) {
                    return TaskCompletion::Passed;
                }
                let result = self
                    .publish_directory::<Reporter>(
                        &root,
                        config,
                        opts,
                        network,
                        before_packing_hooks,
                    )
                    .await;
                record_publish_outcome(published, first_error, result)
            }
        };
        let on_node_skipped: fn(&PathBuf) = |_| {};
        schedule_graph_async(
            project_dependencies,
            &ScheduleGraphAsyncOptions::new(1, true, &run_node, &on_node_skipped),
        )
        .await;
        if let Some(error) = first_error.into_inner().expect("publish error lock is not poisoned") {
            return Err(error);
        }
        Ok(published.into_inner().expect("publish results lock is not poisoned"))
    }
    /// The selected projects that should be published: those with a name
    /// and version, not private, and — unless `--force` — not already on
    /// their registry.
    ///
    /// The already-published probes are independent registry reads, so
    /// they run concurrently rather than one round trip at a time (the
    /// `ThrottledClient` still bounds the actual in-flight fan-out).
    async fn projects_to_publish(
        &self,
        graph: &pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'_>>,
        config: &Config,
        http_client: &pnpm_network::ThrottledClient,
        retry_opts: pnpm_network::RetryOpts,
    ) -> HashSet<PathBuf> {
        let probes = graph.iter().filter_map(|(root, node)| {
            let manifest = node.package.project.manifest.value();
            let (name, version) = publish_eligible(manifest)?;
            Some(async move {
                let already = !self.flags.force
                    && is_already_published(
                        name,
                        version,
                        manifest,
                        config,
                        http_client,
                        retry_opts,
                    )
                    .await;
                (root, already)
            })
        });
        futures_util::future::join_all(probes)
            .await
            .into_iter()
            .filter(|(_, already)| !already)
            .map(|(root, _)| root.clone())
            .collect()
    }

    /// Publish every package the `--filter` selectors select, in dependency
    /// order. Git checks have already run once for the workspace in
    /// [`PublishArgs::run`]; each per-package publish runs with git checks off.
    /// `--report-summary` writes what the run published to the workspace
    /// root.
    fn write_summary(
        &self,
        workspace_root: &Path,
        published: &[PublishSummary],
    ) -> miette::Result<()> {
        if !self.flags.report_summary {
            return Ok(());
        }
        write_publish_summary(workspace_root, published)
    }
}

/// A package's `(published name, version)` when it is eligible to be published,
/// or `None` when it should be skipped before any registry lookup: an unnamed,
/// unversioned, or private package is never published recursively. The name is
/// the one the registry knows — the `publishConfig.name` rename, when the
/// project has one — since it is only used to address the registry.
fn publish_eligible(manifest: &Value) -> Option<(&str, &str)> {
    if manifest.get("private").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let name = manifest.get("name").and_then(Value::as_str)?;
    let version = manifest.get("version").and_then(Value::as_str)?;
    Some((published_name(manifest).unwrap_or(name), version))
}

/// Whether `name@version` already exists on its target registry. Any failure —
/// a 404 for a brand-new package, a transient network error — is treated as
/// "not published" (a failed resolve means the version is absent, so the
/// publish proceeds).
async fn is_already_published(
    name: &str,
    version: &str,
    manifest: &Value,
    config: &Config,
    http_client: &ThrottledClient,
    retry_opts: RetryOpts,
) -> bool {
    let publish_config_registry = manifest
        .get("publishConfig")
        .and_then(|publish_config| publish_config.get("registry"))
        .and_then(Value::as_str);
    let Ok(registry) = find_registry_info(
        name,
        &config.registry,
        &config.registries_by_scope,
        publish_config_registry,
    ) else {
        return false;
    };
    let outcome = fetch_full_metadata(
        name,
        &FetchFullMetadataOptions {
            registry: registry.as_str(),
            http_client,
            auth_headers: &config.auth_headers,
            full_metadata: false,
            etag: None,
            modified: None,
            retry_opts,
        },
    )
    .await;
    matches!(outcome, Ok(FetchFullMetadataOutcome::Modified(package)) if package.versions.contains_key(version))
}

/// Write `pnpm-publish-summary.json` under `dir` with the `{ publishedPackages }`
/// shape pnpm emits for `--report-summary`.
fn write_publish_summary(dir: &Path, published: &[PublishSummary]) -> miette::Result<()> {
    let path = dir.join("pnpm-publish-summary.json");
    let body = serde_json::json!({ "publishedPackages": published });
    let json = body.pipe_ref(serde_json::to_string_pretty).into_diagnostic()?;
    // Write atomically (temp file + rename), matching pnpm's `writeJsonFile`:
    // the target sits under the repo-controlled workspace root, and a
    // non-atomic `std::fs::write` would follow a symlink planted there and
    // could leave a truncated file on a mid-write crash.
    pnpm_fs::write_atomic(&path, json.as_bytes())
        .into_diagnostic()
        .wrap_err_with(|| format!("write {}", path.display()))
}

fn retry_opts_from_config(config: &Config) -> RetryOpts {
    RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
    }
}

/// Emit on the generic `pnpm` channel with a project prefix (rather than the
/// prefix-less `pnpm:global` channel), so the message carries the project dir.
fn emit_info<Reporter: self::Reporter>(message: &str, prefix: &Path) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: message.to_owned(),
        prefix: prefix.display().to_string(),
    }));
}

#[cfg(test)]
mod tests;

/// Record one project's publish. The run does not bail, so a failure
/// only fails that project; the first error is the one reported.
fn record_publish_outcome(
    published: &Mutex<Vec<PublishSummary>>,
    first_error: &Mutex<Option<miette::Report>>,
    result: miette::Result<PublishSummary>,
) -> TaskCompletion {
    match result {
        Ok(summary) => {
            published.lock().expect("publish results lock is not poisoned").push(summary);
            TaskCompletion::Passed
        }
        Err(error) => {
            first_error.lock().expect("publish error lock is not poisoned").get_or_insert(error);
            TaskCompletion::Failed
        }
    }
}
