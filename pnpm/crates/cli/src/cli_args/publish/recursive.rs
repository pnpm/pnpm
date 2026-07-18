//! `pacquet publish --recursive` — publish every selected workspace package.
//!
//! Selects the workspace projects the `--filter` selectors pick, drops the
//! private / unnamed / already-published ones (unless `--force`), then
//! publishes the rest one dependency-ordered chunk at a time, optionally
//! writing `pnpm-publish-summary.json`.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use miette::{Context, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{RetryOpts, ThrottledClient};
use pacquet_publish::{
    Host, PublishNetwork, PublishSummary, find_registry_info, get_current_branch,
    resolve_otp_from_env,
};
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pacquet_resolving_npm_resolver::{
    FetchFullMetadataOptions, FetchFullMetadataOutcome, fetch_full_metadata,
};
use pacquet_versioning::{
    AssembleReleasePlanOptions, assemble_release_plan, read_change_intents, read_ledger,
    to_project_dir,
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
    /// [`PublishArgs::run`]; each per-package publish runs with git checks off.
    pub(super) async fn run_recursive<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        stage: bool,
    ) -> miette::Result<Vec<PublishSummary>> {
        if self.flags.batch {
            return Err(miette::miette!(
                help = "Publish without --batch.",
                "Batch publishing (--batch) is not yet supported",
            ));
        }

        let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
        // `publish` is not in pnpm's root-auto-exclusion command set
        // (`run` / `exec` / `add` / `test`), so the workspace root stays in the
        // selection; its own name/version/private eligibility check drops it
        // below.
        let (projects, _patterns) = discover_workspace_projects(workspace_root)?;
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

        let snapshot_plan = self
            .flags
            .snapshot
            .as_deref()
            .map(|tag| build_snapshot_plan(tag, workspace_root, &projects, graph, config))
            .transpose()?;
        let http_client = build_registry_client(config)?;
        let network = PublishNetwork { client: &http_client, auth_headers: &config.auth_headers };
        let otp = resolve_otp_from_env::<Host>(self.flags.otp.clone());
        let mut opts = self.publish_options(config, otp, stage);
        if let Some(snapshot) = &snapshot_plan {
            opts.tag = snapshot.tag.clone();
            opts.lane = Some(snapshot.tag.clone());
        }
        let retry_opts = retry_opts_from_config(config);

        // Filter the selected graph: keep only packages that have a name and
        // version, are not private, and — unless `--force` — are not already on
        // their registry. The already-published probes are independent registry
        // reads, so run them concurrently rather than one round-trip at a time
        // (the `ThrottledClient` still bounds the actual in-flight fan-out).
        let http_client_ref = &http_client;
        let is_snapshot = snapshot_plan.is_some();
        let probes = graph.iter().filter_map(|(root, node)| {
            if snapshot_plan.as_ref().is_some_and(|snapshot| !snapshot.project_roots.contains(root))
            {
                return None;
            }
            let manifest = node.package.project.manifest.value();
            let (name, version) = publish_eligible(manifest)?;
            Some(async move {
                let already = !is_snapshot
                    && !self.flags.force
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
            if self.flags.report_summary {
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
                let summary = self
                    .publish_directory::<Reporter>(
                        &root,
                        config,
                        &opts,
                        &network,
                        snapshot_plan.as_ref().map(|snapshot| &snapshot.workspace_versions),
                    )
                    .await?;
                published.push(summary);
            }
        }

        if self.flags.report_summary {
            write_publish_summary(workspace_root, &published)?;
        }
        Ok(published)
    }
}

struct SnapshotPlan {
    project_roots: HashSet<PathBuf>,
    tag: String,
    workspace_versions: HashMap<String, String>,
}

fn build_snapshot_plan(
    requested_tag: &str,
    workspace_root: &Path,
    projects: &[pacquet_workspace::Project],
    selected: &pacquet_workspace_projects_graph::ProjectGraph<crate::cli_args::recursive::GraphPkg>,
    config: &Config,
) -> miette::Result<SnapshotPlan> {
    let raw_tag = if requested_tag.is_empty() {
        get_current_branch::<Host>(workspace_root).unwrap_or_else(|| "snapshot".to_string())
    } else {
        requested_tag.to_string()
    };
    let tag = normalize_snapshot_tag(&raw_tag)?;
    let suffix = format!("{}-{}", tag, chrono::Utc::now().format("%Y%m%d%H%M%S"));
    let engine_projects = crate::cli_args::change::to_engine_projects(projects);
    let filter = (!config.filter.is_empty()).then(|| {
        selected.keys().map(|root| to_project_dir(workspace_root, root)).collect::<HashSet<_>>()
    });
    let plan = assemble_release_plan(
        &engine_projects,
        workspace_root,
        &read_change_intents(workspace_root)?,
        &read_ledger(workspace_root)?,
        Some(&config.versioning),
        &AssembleReleasePlanOptions {
            filter,
            snapshot_suffix: Some(suffix),
            enforce_workspace_protocol: true,
        },
    )?;
    Ok(SnapshotPlan {
        project_roots: plan.releases.iter().map(|release| release.root_dir.clone()).collect(),
        workspace_versions: plan
            .releases
            .iter()
            .map(|release| (release.name.clone(), release.new_version.clone()))
            .collect(),
        tag,
    })
}

fn normalize_snapshot_tag(tag: &str) -> miette::Result<String> {
    let mut normalized = String::new();
    let mut last_was_dash = false;
    for character in tag.chars() {
        if character.is_ascii_alphanumeric() || character == '-' {
            normalized.push(character);
            last_was_dash = character == '-';
        } else if !last_was_dash && !normalized.is_empty() {
            normalized.push('-');
            last_was_dash = true;
        }
    }
    let normalized = normalized.trim_matches('-').to_string();
    if normalized.is_empty() {
        return Err(miette::miette!(r#"Cannot derive a snapshot tag from "{tag}""#));
    }
    Ok(normalized)
}

/// A package's `(name, version)` when it is eligible to be published, or `None`
/// when it should be skipped before any registry lookup: an unnamed,
/// unversioned, or private package is never published recursively.
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
    // Write atomically (temp file + rename), matching pnpm's `writeJsonFile`:
    // the target sits under the repo-controlled workspace root, and a
    // non-atomic `std::fs::write` would follow a symlink planted there and
    // could leave a truncated file on a mid-write crash.
    pacquet_fs::write_atomic(&path, json.as_bytes())
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
