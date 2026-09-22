pub(crate) use lockfile_guard::LockfileGuard;

mod lockfile_guard;

use crate::{
    State,
    cli_args::{
        deps_tree::render::{
            TreeNode, blue_bright_underline, gray, green, plain, red, render_archy,
        },
        install::workspace_install_selection,
        pipelines::InstallFamilySelection,
    },
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, PkgNameVerPeer};
use pnpm_modules_yaml::{Host, read_modules_manifest};
use pnpm_package_manager::{
    ImporterDiffKey, InstallabilityHost, LockfileDiff, PolicyExcludes, ResolutionObserver,
    ResolvedPackageHint, SnapshotDiff, diff_lockfiles, package_metadata_is_installable,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::{
    DedupeCheckLog, LogEvent, LogLevel, PnpmErrorLog, ProgressLog, ProgressMessage, Reporter,
};
use pnpm_store_dir::{SharedReadonlyStoreIndex, StoreIndex, store_index_key};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, HashSet},
    marker::PhantomData,
    path::Path,
    sync::Arc,
};

#[derive(Debug, Clone, Args)]
pub struct DedupeArgs {
    /// Check if running dedupe would result in changes without installing
    /// packages or editing the lockfile. Exits with a non-zero status code
    /// if changes are possible.
    #[clap(long)]
    pub check: bool,
    /// Only update `pnpm-lock.yaml`. Don't download packages or write
    /// `node_modules`.
    #[clap(long = "lockfile-only")]
    pub lockfile_only: bool,
    #[clap(flatten)]
    pub scripts: crate::cli_args::install_options::ScriptExecutionArgs,
    #[clap(flatten)]
    pub network_cache: crate::cli_args::install_options::OfflineArgs,
}

impl DedupeArgs {
    pub(crate) fn apply_cli_config(&self, config: &mut Config) {
        self.scripts.apply(config);
        self.network_cache.apply(config);
    }

    /// Run the deduplication install pipeline. In `--check` mode the method
    /// receives a pre-computed snapshot (`existing`) and drop guard created by
    /// the caller *before* config-dependency steps, so the gate covers any
    /// lockfile mutations made by config-deps as well.
    pub(super) async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        existing: Option<String>,
        guard: Option<LockfileGuard>,
        lockfile_path: &Path,
        selection: Option<&InstallFamilySelection>,
    ) -> miette::Result<()> {
        let install = {
            let mut base_install = state.install([
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ]);
            base_install.lockfile_policy.prefer_frozen = Some(false);
            base_install.lockfile_policy.excludes =
                if self.check { PolicyExcludes::Skip } else { PolicyExcludes::Persist };
            base_install.execution.skip_runtimes = false;
            base_install.execution.lockfile_only = self.lockfile_only || self.check;
            base_install.resolution.update_seed_policy =
                pnpm_package_manager::UpdateSeedPolicy::KeepAllResolveAll;
            // Resolve-time store reporting is the only reuse signal for
            // `--lockfile-only`/`--check` runs, which skip the fetch and
            // materialization phases. A full dedupe reports the same store
            // hits again from those phases, so the observer stays quiet
            // there to avoid counting every reused package twice.
            let report_store_hits = self.lockfile_only || self.check;
            base_install.resolution.observer =
                Some(Arc::new(DedupeResolutionReporter::<Reporter>::new(
                    &state,
                    lockfile_path,
                    report_store_hits,
                )?));
            base_install.context.lockfile_path = Some(lockfile_path);
            base_install
        };
        let selection = selection.map(workspace_install_selection);
        if self.check {
            install.run_lockfile_check::<Reporter>(selection).await
        } else if let Some(selection) = selection {
            install.run_selected::<Reporter>(selection).await
        } else {
            install.run::<Reporter>().await
        }
        .wrap_err("deduplicating dependencies")?;

        if self.check {
            check_lockfile::<Reporter>(existing.as_deref(), guard.unwrap(), lockfile_path)
        } else {
            Ok(())
        }
    }
}

pub(crate) fn check_lockfile<Reporter: self::Reporter>(
    existing: Option<&str>,
    mut guard: LockfileGuard,
    lockfile_path: &Path,
) -> miette::Result<()> {
    let current = read_lockfile_snapshot(lockfile_path)?;
    if existing == current.as_deref() {
        guard.disarm();
        return Ok(());
    }
    let diff = diff_lockfiles(
        parse_snapshot(existing, lockfile_path).as_ref(),
        parse_snapshot(current.as_deref(), lockfile_path).as_ref(),
        ImporterDiffKey::Version,
    );
    emit_dedupe_check_error::<Reporter>(&diff);
    Err(DedupeError::CheckIssues.into())
}

#[derive(Debug, Display, Error, Diagnostic)]
enum DedupeError {
    #[display("Dedupe --check found changes to the lockfile")]
    #[diagnostic(code(ERR_PNPM_DEDUPE_CHECK_ISSUES))]
    CheckIssues,
}

/// The skipped optional packages `.modules.yaml` records that the lockfile
/// still holds and this host still cannot install.
fn reusable_skipped_package_ids(
    config: &Config,
    lockfile_packages: Option<&HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::PackageMetadata>>,
) -> miette::Result<HashSet<String>> {
    let modules_manifest = read_modules_manifest::<Host>(&config.modules_dir).into_diagnostic()?;
    let mut installability_host =
        InstallabilityHost::detect_with(config.engine_strict, config.node_version.clone());
    installability_host.supported_architectures.clone_from(&config.supported_architectures);
    Ok(modules_manifest
        .into_iter()
        .flat_map(|modules| modules.skipped)
        .filter_map(|package_id| {
            let package_key = package_id
                .parse::<PkgNameVerPeer>()
                .ok()?
                .without_peer();
            let metadata = lockfile_packages?.get(&package_key)?;
            Some(reusable_skipped_package_id(
                &package_key,
                metadata,
                &installability_host,
                config.ignored_optional_dependencies.as_deref(),
            ))
        })
        .collect::<miette::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

struct DedupeResolutionReporter<Reporter> {
    requester: String,
    store_index: Option<SharedReadonlyStoreIndex>,
    reusable_skipped_package_ids: HashSet<String>,
    /// Whether `on_resolved` reports packages found in the store.
    ///
    /// Enabled for `--lockfile-only`/`--check` runs, which skip the fetch
    /// and materialization phases, so resolve-time reporting is the only
    /// reuse signal. Disabled for full runs: those phases report the same
    /// store hits (deduplicated against each other), and emitting here as
    /// well would count every reused package twice (pnpm/pnpm#15303).
    report_store_hits: bool,
    reporter: PhantomData<fn() -> Reporter>,
}

impl<Reporter> DedupeResolutionReporter<Reporter> {
    fn new(state: &State, lockfile_path: &Path, report_store_hits: bool) -> miette::Result<Self> {
        let config = state.config;
        let lockfile_packages = state.lockfile
            .get()
            .into_diagnostic()?
            .and_then(|lockfile| lockfile.packages.as_ref());
        let reusable_skipped_package_ids = reusable_skipped_package_ids(config, lockfile_packages)?;
        Ok(Self {
            requester: lockfile_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .display()
                .to_string(),
            store_index: StoreIndex::shared_for(&config.store_dir, config.frozen_store),
            reusable_skipped_package_ids,
            report_store_hits,
            reporter: PhantomData,
        })
    }
}

impl<Reporter: self::Reporter> ResolutionObserver for DedupeResolutionReporter<Reporter> {
    fn on_resolved(&self, hint: ResolvedPackageHint<'_>) {
        Reporter::emit(&LogEvent::Progress(ProgressLog {
            level: LogLevel::Debug,
            message: ProgressMessage::Resolved {
                package_id: hint.identity.id.to_string(),
                requester: self.requester.clone(),
            },
        }));
        if !self.report_store_hits {
            return;
        }
        let package_key = store_index_key(hint.integrity, hint.identity.id);
        let found_in_store = self.reusable_skipped_package_ids.contains(hint.identity.id)
            || self.store_index
                .as_ref()
                .is_some_and(|store_index| {
                    store_index
                        .lock()
                        .ok()
                        .and_then(|index| index.contains_key(&package_key).ok())
                        .unwrap_or(false)
                });
        if found_in_store {
            Reporter::emit(&LogEvent::Progress(ProgressLog {
                level: LogLevel::Debug,
                message: ProgressMessage::FoundInStore {
                    package_id: hint.identity.id.to_string(),
                    requester: self.requester.clone(),
                },
            }));
        }
    }
}

fn reusable_skipped_package_id(
    package_key: &PkgNameVerPeer,
    metadata: &pnpm_lockfile::PackageMetadata,
    installability_host: &InstallabilityHost,
    ignored_optional_dependencies: Option<&[String]>,
) -> miette::Result<Option<String>> {
    if ignored_optional_dependencies.is_some_and(|ignored| {
        ignored.contains(&package_key.name.to_string())
    }) {
        return Ok(None);
    }
    Ok(package_metadata_is_installable(package_key, metadata, installability_host)
        .into_diagnostic()?
        .then(|| package_key.pkg_id()))
}

fn emit_dedupe_check_error<Reporter: self::Reporter>(diff: &LockfileDiff) {
    let message = "Dedupe --check found changes to the lockfile".to_string();
    Reporter::emit(&LogEvent::DedupeCheck(DedupeCheckLog {
        level: LogLevel::Error,
        message: message.clone(),
        err: PnpmErrorLog { code: "ERR_PNPM_DEDUPE_CHECK_ISSUES".to_string(), message },
        dedupe_check_issues: dedupe_check_issues_json(diff),
        rendered: render_dedupe_check_error(diff),
    }));
}

/// Parse one side of the `--check` diff. A snapshot that does not parse —
/// an older lockfile format the dedupe install has just rewritten, say —
/// yields no baseline rather than replacing the check's verdict with a
/// parse error: the run already knows the lockfile would change, and only
/// the detail of the report is lost.
fn parse_snapshot(content: Option<&str>, lockfile_path: &Path) -> Option<Lockfile> {
    content.and_then(|content| Lockfile::parse(content, lockfile_path).ok().flatten())
}

/// Render what `pnpm dedupe` would rewrite, mirroring pnpm's
/// `renderDedupeCheckIssues`: one tree per changed importer or package
/// snapshot, plus the snapshots deduplication would add or drop.
///
/// The lockfile can also be rewritten without any resolution changing —
/// recorded settings drift, a config dependency the run synced — so an
/// empty diff still says why the check failed.
fn render_dedupe_check_issues(diff: &LockfileDiff) -> String {
    if diff.is_empty() {
        return "The lockfile would be rewritten, but no dependency resolution would change."
            .to_string();
    }
    [
        render_section("Importers", &diff.importers, &[], &[]),
        render_section(
            "Packages",
            &diff.updated_packages,
            &diff.added_packages,
            &diff.removed_packages,
        ),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n")
}

fn render_dedupe_check_error(diff: &LockfileDiff) -> String {
    let issues = render_dedupe_check_issues(diff);
    let recommendation_separator = if issues.ends_with("\n\n") {
        ""
    } else if issues.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!(
        "[ERR_PNPM_DEDUPE_CHECK_ISSUES] Dedupe --check found changes to the lockfile\n\n{issues}{recommendation_separator}Run pnpm dedupe to apply the changes above.\n",
    )
}

fn dedupe_check_issues_json(diff: &LockfileDiff) -> Value {
    json!({
        "importerIssuesByImporterId": snapshots_changes_json(&diff.importers, &[], &[]),
        "packageIssuesByDepPath": snapshots_changes_json(
            &diff.updated_packages,
            &diff.added_packages,
            &diff.removed_packages,
        ),
    })
}

fn snapshots_changes_json(updated: &[SnapshotDiff], added: &[String], removed: &[String]) -> Value {
    let updated = updated
        .iter()
        .map(|snapshot| {
            let changes = snapshot.added
                .iter()
                .map(|(alias, next)| (alias.clone(), json!({ "type": "added", "next": next })))
                .chain(
                    snapshot.removed
                        .iter()
                        .map(|(alias, prev)| {
                            (alias.clone(), json!({ "type": "removed", "prev": prev }))
                        }),
                )
                .chain(
                    snapshot.updated
                        .iter()
                        .map(|(alias, prev, next)| {
                            (
                                alias.clone(),
                                json!({ "type": "updated", "prev": prev, "next": next }),
                            )
                        }),
                )
                .collect::<Map<_, _>>();
            (snapshot.id.clone(), Value::Object(changes))
        })
        .collect::<Map<_, _>>();
    json!({
        "added": added,
        "removed": removed,
        "updated": updated,
    })
}

fn render_section(
    title: &str,
    updated: &[SnapshotDiff],
    added: &[String],
    removed: &[String],
) -> Option<String> {
    let mut lines: Vec<String> = updated
        .iter()
        .map(render_snapshot_diff)
        .collect();
    lines.extend(
        added
            .iter()
            .map(|id| format!("{} {}", green("+"), plain(id))),
    );
    lines.extend(
        removed
            .iter()
            .map(|id| format!("{} {}", red("-"), plain(id))),
    );
    if lines.is_empty() {
        return None;
    }
    Some(format!("{}\n{}\n", blue_bright_underline(title), lines.join("\n")))
}

fn render_snapshot_diff(diff: &SnapshotDiff) -> String {
    let added = diff.added
        .iter()
        .map(|(alias, next)| format!("{} {} {}", green("+"), plain(alias), gray(next)));
    let removed = diff.removed
        .iter()
        .map(|(alias, prev)| format!("{} {} {}", red("-"), plain(alias), gray(prev)));
    let updated = diff.updated
        .iter()
        .map(|(alias, prev, next)| {
            format!("{} {} {} {}", plain(alias), red(prev), gray("→"), green(next))
        });
    let nodes = added
        .chain(removed)
        .chain(updated)
        .map(|label| TreeNode::with_children(label, Vec::new()))
        .collect();
    render_archy(&TreeNode::with_children(plain(&diff.id), nodes))
}

/// Read pnpm-lock.yaml into an `Option<String>` for snapshot comparisons.
/// Returns `None` when the file does not exist.
pub(crate) fn read_lockfile_snapshot(lockfile_path: &Path) -> miette::Result<Option<String>> {
    match std::fs::read_to_string(lockfile_path) {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).into_diagnostic().wrap_err("reading lockfile"),
    }
}

#[cfg(test)]
mod tests;
