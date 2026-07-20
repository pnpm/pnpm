use std::{
    io::Write,
    path::{Path, PathBuf},
};

use crate::State;
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pacquet_package_manager::Install;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;
use tempfile::NamedTempFile;

#[derive(Debug, Args)]
pub struct DedupeArgs {
    #[clap(long)]
    pub check: bool,
}

impl DedupeArgs {
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

        Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            emit_initial_manifest: true,
            lockfile: pacquet_lockfile::MaybeLazyLockfile::Lazy(lockfile),
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
            is_full_install: true,
            installs_only: true,
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            node_linker: config.node_linker,
            lockfile_only: true,
            dry_run: false,
            update_seed_policy: pacquet_package_manager::UpdateSeedPolicy::DropAll,
            auth_override: None,
            resolution_observer: None,
            peer_issues_sink: None,
            catalogs_override: None,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        }
        .run::<Reporter>()
        .await
        .wrap_err("deduplicating dependencies")?;

        if self.check {
            let mut guard = guard.unwrap();
            let current = read_lockfile_snapshot(lockfile_path)?;
            if existing == current {
                guard.disarm();
                Ok(())
            } else {
                Err(miette::miette!("Lockfile would be modified by deduplication"))
            }
        } else {
            Ok(())
        }
    }
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
