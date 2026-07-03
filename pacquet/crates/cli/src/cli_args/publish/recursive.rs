//! `pacquet publish --recursive` — publish every selected workspace package.
//!
//! Ports pnpm's
//! [`recursivePublish`](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/recursivePublish.ts).
//! Selects the workspace projects the `--filter` selectors pick, drops the
//! private / unnamed / already-published ones (unless `--force`), then
//! publishes the rest one dependency-ordered chunk at a time, optionally
//! writing `pnpm-publish-summary.json`.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use miette::{Context, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{RetryOpts, ThrottledClient};
use pacquet_publish::{
    Host, PublishNetwork, PublishSummary, find_registry_info, resolve_otp_from_env,
};
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pacquet_resolving_npm_resolver::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, fetch_full_metadata,
};
use pipe_trait::Pipe;
use serde_json::Value;

use super::PublishArgs;
use crate::cli_args::{
    recursive::{
        AutoExcludeRoot, discover_workspace_projects, select_recursive_projects,
        sort_filtered_projects,
    },
    registry_client::build_registry_client,
};

impl PublishArgs {
    /// Publish every package the `--filter` selectors select, in dependency
    /// order. Git checks have already run once for the workspace in
    /// [`PublishArgs::run`]; each per-package publish runs with them off,
    /// matching pnpm's `gitChecks: false` per sub-publish.
    pub(super) async fn run_recursive<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
    ) -> miette::Result<Vec<PublishSummary>> {
        if self.batch {
            return Err(miette::miette!(
                help = "Publish without --batch; batched publishing is not yet ported to pacquet.",
                "Batch publishing (--batch) is not yet supported by pacquet",
            ));
        }

        let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
        // `publish` is not in pnpm's root-auto-exclusion command set
        // (`run` / `exec` / `add` / `test`), so the workspace root stays in the
        // selection; its own name/version/private eligibility check drops it
        // below, matching pnpm's `recursivePublish`.
        let (projects, _patterns) = discover_workspace_projects(workspace_root)?;
        let selection =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
        let graph = &selection.selected;
        // An empty selection is a no-op (exit 0) that writes no summary —
        // whether the workspace enumerates no project at all or a `--filter`
        // narrowed it to nothing. Mirrors pnpm's main.ts dispatch, which
        // returns before the publish handler for both `allProjects.length === 0`
        // and an empty `selectedProjectsGraph`.
        if graph.is_empty() {
            return Ok(Vec::new());
        }

        let http_client = build_registry_client(config)?;
        let network = PublishNetwork { client: &http_client, auth_headers: &config.auth_headers };
        let otp = resolve_otp_from_env::<Host>(self.otp.clone());
        let opts = self.publish_options(config, otp);
        let retry_opts = retry_opts_from_config(config);

        // Mirror pnpm's `pFilter` over the selected graph: keep only packages
        // that have a name and version, are not private, and — unless
        // `--force` — are not already on their registry. The already-published
        // probes are independent registry reads, so run them concurrently like
        // pnpm's `pFilter` instead of one round-trip at a time (the
        // `ThrottledClient` still bounds the actual in-flight fan-out).
        let http_client_ref = &http_client;
        let probes = graph.iter().filter_map(|(root, node)| {
            let manifest = node.package.project.manifest.value();
            let (name, version) = publish_eligible(manifest)?;
            Some(async move {
                let already = !self.force
                    && is_already_published(
                        name,
                        version,
                        manifest,
                        config,
                        http_client_ref,
                        retry_opts,
                    )
                    .await;
                (root, already)
            })
        });
        let to_publish: HashSet<PathBuf> = futures_util::future::join_all(probes)
            .await
            .into_iter()
            .filter(|(_, already)| !already)
            .map(|(root, _)| root.clone())
            .collect();

        if to_publish.is_empty() {
            emit_info::<Reporter>("There are no new packages that should be published", dir);
            if self.report_summary {
                write_publish_summary(workspace_root, &[])?;
            }
            return Ok(Vec::new());
        }

        // Publish chunk by chunk in dependency order. Publishing cannot run
        // concurrently: an OTP challenge is interactive and per-process.
        let mut published: Vec<PublishSummary> = Vec::new();
        let chunks = sort_filtered_projects(
            graph,
            selection.full_graph(),
            selection.prod_all.as_ref(),
            &selection.prod_only_selected,
        );
        for chunk in chunks {
            for root in chunk {
                if !to_publish.contains(&root) {
                    continue;
                }
                let summary =
                    self.publish_directory::<Reporter>(&root, config, &opts, &network).await?;
                published.push(summary);
            }
        }

        if self.report_summary {
            write_publish_summary(workspace_root, &published)?;
        }
        Ok(published)
    }
}

/// A package's `(name, version)` when it is eligible to be published, or `None`
/// when it should be skipped before any registry lookup. Mirrors pnpm's
/// `if (!pkg.manifest.name || !pkg.manifest.version || pkg.manifest.private)
/// return false`: an unnamed, unversioned, or private package is never
/// published recursively.
fn publish_eligible(manifest: &Value) -> Option<(&str, &str)> {
    if manifest.get("private").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let name = manifest.get("name").and_then(Value::as_str)?;
    let version = manifest.get("version").and_then(Value::as_str)?;
    Some((name, version))
}

/// Whether `name@version` already exists on its target registry. Any failure —
/// a 404 for a brand-new package, a transient network error — is treated as
/// "not published", matching pnpm's `isAlreadyPublished` catch-all (a failed
/// resolve means the version is absent, so the publish proceeds).
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
    let Ok(registry) =
        find_registry_info(name, &config.registry, &config.registries, publish_config_registry)
    else {
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
    std::fs::write(&path, json)
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

/// Emit on the generic `pnpm` channel with a project prefix, matching
/// pnpm's `recursivePublish` which surfaces this through
/// `logger.info({ message, prefix: opts.dir })` rather than the
/// prefix-less `globalInfo` (`pnpm:global`) channel.
fn emit_info<Reporter: self::Reporter>(message: &str, prefix: &Path) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: message.to_owned(),
        prefix: prefix.display().to_string(),
    }));
}

#[cfg(test)]
mod tests;
