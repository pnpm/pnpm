use std::{
    collections::HashSet,
    io::Write,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    State,
    cli_args::{
        deps_tree::render::{
            TreeNode, blue_bright_underline, gray, green, plain, red, render_archy,
        },
        install::resolve_bool_override,
    },
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, PkgNameVerPeer};
use pnpm_modules_yaml::{Host, read_modules_manifest};
use pnpm_package_manager::{
    ImporterDiffKey, Install, InstallabilityHost, LockfileDiff, ProjectMutation,
    ResolutionObserver, ResolvedPackageHint, SnapshotDiff, diff_lockfiles,
    package_metadata_is_installable,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::{
    DedupeCheckLog, LogEvent, LogLevel, PnpmErrorLog, ProgressLog, ProgressMessage, Reporter,
};
use pnpm_store_dir::{SharedReadonlyStoreIndex, StoreIndex, store_index_key};
use serde_json::{Map, Value, json};
use tempfile::NamedTempFile;

#[derive(Debug, Args)]
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

    /// Don't run lifecycle scripts of the project or its dependencies.
    /// Packages are still installed; only their build scripts are skipped,
    /// and the install won't fail because of it.
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    pub ignore_scripts: bool,

    /// Run lifecycle scripts even when the configuration disables them.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    pub no_ignore_scripts: bool,

    /// Fail on a cache miss instead of fetching from the registry, using
    /// only packages already in the store.
    #[clap(long, overrides_with = "no_offline")]
    pub offline: bool,

    /// Allow network fetches even when the configuration enables offline
    /// mode.
    #[clap(long = "no-offline", overrides_with = "offline")]
    pub no_offline: bool,

    /// Prefer packages already in the cache over the network, even past
    /// their freshness window.
    #[clap(long, overrides_with = "no_prefer_offline")]
    pub prefer_offline: bool,

    /// Don't prefer cached packages even when the configuration enables
    /// it.
    #[clap(long = "no-prefer-offline", overrides_with = "prefer_offline")]
    pub no_prefer_offline: bool,

    /// Disable pnpm hooks defined in `.pnpmfile.cjs`, including the
    /// pnpmfiles of config dependencies.
    #[clap(long = "ignore-pnpmfile")]
    pub ignore_pnpmfile: bool,
}

impl DedupeArgs {
    pub(crate) fn apply_cli_config(&self, config: &mut Config) {
        config.ignore_pnpmfile = self.ignore_pnpmfile || config.ignore_pnpmfile;
        config.ignore_scripts = resolve_bool_override(
            self.ignore_scripts,
            self.no_ignore_scripts,
            config.ignore_scripts,
        );
        config.offline = resolve_bool_override(self.offline, self.no_offline, config.offline);
        config.prefer_offline = resolve_bool_override(
            self.prefer_offline,
            self.no_prefer_offline,
            config.prefer_offline,
        );
    }

    /// Run the deduplication install pipeline. In `--check` mode the method
    /// receives a pre-computed snapshot (`existing`) and drop guard created by
    /// the caller *before* config-dependency steps, so the gate covers any
    /// lockfile mutations made by config-deps as well.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        existing: Option<String>,
        guard: Option<LockfileGuard>,
        lockfile_path: &Path,
    ) -> miette::Result<()> {
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;
        let lockfile_packages =
            lockfile.get().into_diagnostic()?.and_then(|lockfile| lockfile.packages.as_ref());
        let modules_manifest =
            read_modules_manifest::<Host>(&config.modules_dir).into_diagnostic()?;
        let mut installability_host =
            InstallabilityHost::detect_with(config.engine_strict, config.node_version.clone());
        installability_host.supported_architectures = config.supported_architectures.clone();
        let reusable_skipped_package_ids = modules_manifest
            .into_iter()
            .flat_map(|modules| modules.skipped)
            .filter_map(|package_id| {
                let package_key = package_id.parse::<PkgNameVerPeer>().ok()?.without_peer();
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
            .collect();

        let install = Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            emit_initial_manifest: true,
            lockfile: pnpm_lockfile::MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: Some(lockfile_path),
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ]
            .into_iter(),
            frozen_lockfile: false,
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: false,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            node_linker: config.node_linker,
            lockfile_only: self.lockfile_only || self.check,
            dry_run: false,
            // `--check` must leave the working tree untouched: `lockfile_only`
            // above keeps `node_modules` out of it, the lockfile guard restores
            // `pnpm-lock.yaml`, and this gate keeps loose minimumReleaseAge
            // picks out of `pnpm-workspace.yaml`.
            persist_policy_excludes: !self.check,
            update_seed_policy: pnpm_package_manager::UpdateSeedPolicy::KeepAllResolveAll,
            preferred_versions_override: None,
            auth_override: None,
            resolution_observer: Some(Arc::new(DedupeResolutionReporter::<Reporter> {
                requester: lockfile_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .display()
                    .to_string(),
                store_index: StoreIndex::shared_for(&config.store_dir, config.frozen_store),
                reusable_skipped_package_ids,
                reporter: PhantomData,
            })),
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
            catalogs_override: None,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        };
        if self.check {
            install.run_lockfile_check::<Reporter>().await
        } else {
            install.run::<Reporter>().await
        }
        .wrap_err("deduplicating dependencies")?;

        let current = read_lockfile_snapshot(lockfile_path)?;
        let deduped = parse_snapshot(current.as_deref(), lockfile_path);

        if self.check {
            let mut guard = guard.unwrap();
            if existing == current {
                guard.disarm();
                Ok(())
            } else {
                let diff = diff_lockfiles(
                    parse_snapshot(existing.as_deref(), lockfile_path).as_ref(),
                    deduped.as_ref(),
                    ImporterDiffKey::Version,
                );
                emit_dedupe_check_error::<Reporter>(&diff);
                Err(DedupeError::CheckIssues.into())
            }
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Display, Error, Diagnostic)]
enum DedupeError {
    #[display("Dedupe --check found changes to the lockfile")]
    #[diagnostic(code(ERR_PNPM_DEDUPE_CHECK_ISSUES))]
    CheckIssues,
}

struct DedupeResolutionReporter<Reporter> {
    requester: String,
    store_index: Option<SharedReadonlyStoreIndex>,
    reusable_skipped_package_ids: HashSet<String>,
    reporter: PhantomData<fn() -> Reporter>,
}

impl<Reporter: self::Reporter> ResolutionObserver for DedupeResolutionReporter<Reporter> {
    fn on_resolved(&self, hint: ResolvedPackageHint<'_>) {
        Reporter::emit(&LogEvent::Progress(ProgressLog {
            level: LogLevel::Debug,
            message: ProgressMessage::Resolved {
                package_id: hint.id.to_string(),
                requester: self.requester.clone(),
            },
        }));
        let package_key = store_index_key(hint.integrity, hint.id);
        let found_in_store = self.reusable_skipped_package_ids.contains(hint.id)
            || self.store_index.as_ref().is_some_and(|store_index| {
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
                    package_id: hint.id.to_string(),
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
    if ignored_optional_dependencies
        .is_some_and(|ignored| ignored.contains(&package_key.name.to_string()))
    {
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
            let changes = snapshot
                .added
                .iter()
                .map(|(alias, next)| (alias.clone(), json!({ "type": "added", "next": next })))
                .chain(snapshot.removed.iter().map(|(alias, prev)| {
                    (alias.clone(), json!({ "type": "removed", "prev": prev }))
                }))
                .chain(snapshot.updated.iter().map(|(alias, prev, next)| {
                    (alias.clone(), json!({ "type": "updated", "prev": prev, "next": next }))
                }))
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
    let mut lines: Vec<String> = updated.iter().map(render_snapshot_diff).collect();
    lines.extend(added.iter().map(|id| format!("{} {}", green("+"), plain(id))));
    lines.extend(removed.iter().map(|id| format!("{} {}", red("-"), plain(id))));
    if lines.is_empty() {
        return None;
    }
    Some(format!("{}\n{}\n", blue_bright_underline(title), lines.join("\n")))
}

fn render_snapshot_diff(diff: &SnapshotDiff) -> String {
    let added = diff
        .added
        .iter()
        .map(|(alias, next)| format!("{} {} {}", green("+"), plain(alias), gray(next)));
    let removed = diff
        .removed
        .iter()
        .map(|(alias, prev)| format!("{} {} {}", red("-"), plain(alias), gray(prev)));
    let updated = diff.updated.iter().map(|(alias, prev, next)| {
        format!("{} {} {} {}", plain(alias), red(prev), gray("→"), green(next))
    });
    let nodes = added
        .chain(removed)
        .chain(updated)
        .map(|label| TreeNode::with_children(label, Vec::new()))
        .collect();
    render_archy(&TreeNode::with_children(plain(&diff.id), nodes))
}

/// Atomically write `content` to `path` via temp-file + rename, so the write
/// does not follow symlinks and cannot produce a torn file on crash.
fn atomic_write(path: &Path, content: &[u8]) -> miette::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = NamedTempFile::new_in(dir)
        .into_diagnostic()
        .wrap_err("creating temp file for atomic write")?;
    tmp.write_all(content).into_diagnostic().wrap_err("writing temp file")?;
    tmp.as_file().sync_all().into_diagnostic().wrap_err("syncing temp file")?;
    tmp.persist(path).into_diagnostic().wrap_err("renaming temp file into place")?;
    Ok(())
}

/// A drop guard for `--check` mode: restores the lockfile snapshot on drop
/// unless [`disarm`](LockfileGuard::disarm) has been called. This way an
/// unexpected error during deduplication still leaves the workspace in its
/// original state.
pub(crate) struct LockfileGuard {
    existing: Option<String>,
    lockfile_path: PathBuf,
    disarmed: bool,
}

impl LockfileGuard {
    pub(crate) fn new(existing: Option<String>, lockfile_path: &Path) -> Self {
        Self { existing, lockfile_path: lockfile_path.to_path_buf(), disarmed: false }
    }

    pub(crate) fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for LockfileGuard {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        match self.existing.take() {
            Some(ref old) => {
                let _ = atomic_write(&self.lockfile_path, old.as_bytes());
            }
            None => {
                let _ = std::fs::remove_file(&self.lockfile_path);
            }
        }
    }
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
