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
use render::{dedupe_check_issues_json, parse_snapshot, render_dedupe_check_error};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::NamedTempFile;

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
            base_install.lockfile_policy.excludes = if self.check {
                PolicyExcludes::Skip
            } else {
                PolicyExcludes::Persist
            };
            base_install.execution.skip_runtimes = false;
            base_install.execution.lockfile_only =
                self.lockfile_only || self.check;
            base_install.resolution.update_seed_policy =
                pnpm_package_manager::UpdateSeedPolicy::KeepAllResolveAll;
            base_install.resolution.observer = Some(Arc::new(
                DedupeResolutionReporter::<Reporter>::new(&state, lockfile_path)?,
            ));
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
    reporter: PhantomData<fn() -> Reporter>,
}

impl<Reporter> DedupeResolutionReporter<Reporter> {
    fn new(state: &State, lockfile_path: &Path) -> miette::Result<Self> {
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
    Ok(
        package_metadata_is_installable(package_key, metadata, installability_host)
            .into_diagnostic()?
            .then(|| package_key.pkg_id()),
    )
}

fn emit_dedupe_check_error<Reporter: self::Reporter>(diff: &LockfileDiff) {
    let message = "Dedupe --check found changes to the lockfile".to_string();
    Reporter::emit(&LogEvent::DedupeCheck(DedupeCheckLog {
        level: LogLevel::Error,
        message: message.clone(),
        err: PnpmErrorLog {
            code: "ERR_PNPM_DEDUPE_CHECK_ISSUES".to_string(),
            message,
        },
        dedupe_check_issues: dedupe_check_issues_json(diff),
        rendered: render_dedupe_check_error(diff),
    }));
}

/// Atomically write `content` to `path` via temp-file + rename, so the write
/// does not follow symlinks and cannot produce a torn file on crash.
fn atomic_write(path: &Path, content: &[u8]) -> miette::Result<()> {
    let dir = path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = NamedTempFile::new_in(dir)
        .into_diagnostic()
        .wrap_err("creating temp file for atomic write")?;
    tmp
        .write_all(content)
        .into_diagnostic()
        .wrap_err("writing temp file")?;
    tmp
        .as_file()
        .sync_all()
        .into_diagnostic()
        .wrap_err("syncing temp file")?;
    tmp
        .persist(path)
        .into_diagnostic()
        .wrap_err("renaming temp file into place")?;
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
        Self {
            existing,
            lockfile_path: lockfile_path.to_path_buf(),
            disarmed: false,
        }
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

mod render;
