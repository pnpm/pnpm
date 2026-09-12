pub(crate) mod add;

pub(crate) use lockfile::workspace_root;
pub(crate) use sparse_registry::{cargo_auth_headers, latest_version};

use crate::{
    cargo_deps::git::{GIT_SOURCE_DIRECTORY, GIT_SOURCE_NAME, GitPackage, GitSource},
    ecosystem_install::{EcosystemManifest, EcosystemWorkspaceInventory, InstallContext},
};
use cargo_util_schemas::index::RegistryConfig;
use futures_util::{StreamExt, TryStreamExt, stream};

use lockfile::{
    LockedCrate, discover_workspace_roots, parse_lockfile, read_or_resolve_lockfile,
    validate_package_field,
};
use materialize::{DownloadOptions, add_cargo_checksum, download_crates};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_cargo_resolver::is_crates_io;
use pnpm_config::Config;
use pnpm_deps_restorer::{ImportIndexedDirOpts, import_indexed_dir};
use pnpm_install_coordinator::{InstallTask, PreparedInstall};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_pnpr_client::{CargoResolveOptions, PnprClient};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{
    CafsFileInfo, SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex,
    StoreIndexWriter,
};
use pnpm_tarball::{ArchiveStoreProjection, IngestTarballToStore};
use serde::{Deserialize, Serialize};

use sparse_registry::{fetch_sparse_index, registry_download_config};
use ssri::{Algorithm, Integrity};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{Arc, atomic::AtomicU8},
};
#[cfg(all(test, windows))]
use workspace_directory::ensure_workspace_directory_windows;
use workspace_directory::{
    ManagedDirectory, ensure_workspace_directory, force_workspace_symlink, read_workspace_file,
    write_workspace_file,
};

mod checksum_cache;
mod git;
mod registry_auth;

const WORKSPACE_INSTALL_CONCURRENCY: usize = 8;

const MANAGED_START: &str = "# >>> pnpm-managed cargo sources >>>";
const MANAGED_END: &str = "# <<< pnpm-managed cargo sources <<<";
/// Directory the registry crates are linked into, relative to the Cargo
/// workspace root.
const CRATES_SOURCE_DIRECTORY: [&str; 3] = [".pnpm", "crates", "crates-io"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoLockfilePolicy {
    UseExisting,
    Resolve,
}

#[derive(Deserialize)]
struct CargoWorkspaceMetadata {
    workspace_root: PathBuf,
    packages: Vec<CargoWorkspacePackage>,
}

#[derive(Deserialize)]
struct CargoWorkspacePackage {
    manifest_path: PathBuf,
}

pub(crate) async fn plan<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    inventory: &EcosystemWorkspaceInventory,
) -> Result<InstallTask<'static>> {
    let roots =
        discover_workspace_roots(inventory.manifests(EcosystemManifest::Cargo).await?).await?;
    let metadata = roots.iter().flat_map(|root| metadata_paths(root)).collect();
    Ok(InstallTask::new(
        metadata,
        prepare::<Reporter>(context, roots, CargoLockfilePolicy::UseExisting),
    ))
}

pub(crate) fn metadata_paths(root: &Path) -> [PathBuf; 2] {
    [root.join("Cargo.lock"), root.join(".cargo/config.toml")]
}

pub(crate) async fn prepare<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    roots: Vec<PathBuf>,
    lockfile_policy: CargoLockfilePolicy,
) -> Result<Vec<Prepared>> {
    let InstallContext { config, http_client, lockfile_only, frozen_lockfile } = context;
    let prepares = stream::iter(roots).map(|root| {
        let http_client = Arc::clone(&http_client);
        async move {
            prepare_workspace::<Reporter>(
                config,
                &root,
                lockfile_only,
                frozen_lockfile,
                lockfile_policy,
                http_client,
            )
            .await
        }
    });
    let mut prepared = join_all_buffered(prepares, WORKSPACE_INSTALL_CONCURRENCY).await?;
    prepared.sort_by(|left, right| left.root.cmp(&right.root));
    Ok(prepared)
}

/// Every future's result, at most `concurrency` of them in flight. The
/// whole batch is awaited, so a failing future does not cancel the rest,
/// and the first error in completion order is the one returned.
pub(super) async fn join_all_buffered<Fut, Item>(
    futures: impl stream::Stream<Item = Fut>,
    concurrency: usize,
) -> Result<Vec<Item>>
where
    Fut: std::future::Future<Output = Result<Item>>,
{
    let settled: Vec<_> = futures.buffer_unordered(concurrency).collect().await;
    settled.into_iter().collect()
}

pub(crate) struct Prepared {
    root: PathBuf,
    lock: String,
    slots: Option<WorkspaceSlots>,
    index_url: String,
}

/// The store slots a workspace links, one list per Cargo source the
/// managed configuration declares.
#[derive(Debug, Default)]
struct WorkspaceSlots {
    crates: Vec<(String, PathBuf)>,
    git: Vec<(String, PathBuf)>,
    git_sources: Vec<GitSource>,
}

impl PreparedInstall for Prepared {
    fn publish(&mut self) -> Result<()> {
        if let Some(slots) = &self.slots {
            link_workspace(&self.root, &CRATES_SOURCE_DIRECTORY, &slots.crates)?;
            if !slots.git.is_empty() {
                link_workspace(&self.root, &GIT_SOURCE_DIRECTORY, &slots.git)?;
            }
            write_cargo_config(&self.root, &self.index_url, &slots.git_sources)?;
        }
        let path = self.root.join("Cargo.lock");
        let existing = match fs::read_to_string(&path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).into_diagnostic(),
        };
        if existing.as_deref() != Some(&self.lock) {
            pnpm_fs::write_atomic(&path, self.lock.as_bytes())
                .into_diagnostic()
                .wrap_err_with(|| format!("write {}", path.display()))?;
        }
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        // Versioned source links are additive cache entries. Restoring the
        // declared lockfile and source configuration restores project selection.
        Ok(())
    }

    fn retain(self: Box<Self>) {}
}

async fn prepare_workspace<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    root_dir: &Path,
    lockfile_only: bool,
    frozen_lockfile: bool,
    lockfile_policy: CargoLockfilePolicy,
    http_client: Arc<ThrottledClient>,
) -> Result<Prepared> {
    let cargo_lock_path = root_dir.join("Cargo.lock");
    let cargo_lock = read_or_resolve_lockfile(
        config,
        root_dir,
        &cargo_lock_path,
        frozen_lockfile,
        lockfile_policy,
        &http_client,
    )
    .await?;
    if lockfile_only {
        return Ok(Prepared {
            root: root_dir.to_path_buf(),
            lock: cargo_lock,
            slots: None,
            index_url: config.cargo.index_url.clone(),
        });
    }
    let slots = prepare_workspace_slots::<Reporter>(
        config,
        root_dir,
        &cargo_lock_path,
        &cargo_lock,
        http_client,
    )
    .await?;

    Ok(Prepared {
        root: root_dir.to_path_buf(),
        lock: cargo_lock,
        slots: Some(slots),
        index_url: config.cargo.index_url.clone(),
    })
}

fn link_workspace(root_dir: &Path, directory: &[&str], slots: &[(String, PathBuf)]) -> Result<()> {
    let source_dir = ensure_workspace_directory(root_dir, directory)?;
    link_workspace_in(&source_dir, slots)
}

fn link_workspace_in(source_dir: &ManagedDirectory, slots: &[(String, PathBuf)]) -> Result<()> {
    for (name, slot) in slots {
        let outcome = force_workspace_symlink(source_dir, slot, name)
            .into_diagnostic()
            .wrap_err_with(|| format!("link cargo package {name}"))?;
        if let Some(warning) = outcome.warning {
            tracing::warn!(target: "pacquet::cargo", ?warning, "cargo package link warning");
        }
    }
    Ok(())
}

fn write_cargo_config(root_dir: &Path, index_url: &str, git_sources: &[GitSource]) -> Result<()> {
    let cargo_dir = ensure_workspace_directory(root_dir, &[".cargo"])?;
    write_cargo_config_in(&cargo_dir, index_url, git_sources)
}

fn write_cargo_config_in(
    cargo_dir: &ManagedDirectory,
    index_url: &str,
    git_sources: &[GitSource],
) -> Result<()> {
    let config_path = cargo_dir.path.join("config.toml");
    let (existing, mode) = match read_workspace_file(cargo_dir, "config.toml") {
        Ok(existing) => existing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => (String::new(), None),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", config_path.display()));
        }
    };
    let updated = update_managed_config(&existing, index_url, git_sources)?;
    if updated != existing {
        write_workspace_file(cargo_dir, "config.toml", updated.as_bytes(), mode)
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", config_path.display()))?;
    }
    Ok(())
}

fn update_managed_config(
    existing: &str,
    index_url: &str,
    git_sources: &[GitSource],
) -> Result<String> {
    let managed_config = managed_config(index_url, git_sources);
    match (existing.find(MANAGED_START), existing.find(MANAGED_END)) {
        (None, None) => {
            let separator = if existing.is_empty() || existing.ends_with("\n\n") {
                ""
            } else if existing.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            Ok(format!("{existing}{separator}{managed_config}\n"))
        }
        (Some(start), Some(end)) if start <= end => {
            let after = end + MANAGED_END.len();
            Ok(format!("{}{}{}", &existing[..start], managed_config, &existing[after..]))
        }
        _ => Err(miette::miette!(
            ".cargo/config.toml contains an incomplete pnpm-managed Cargo source block"
        )),
    }
}

fn managed_config(index_url: &str, git_sources: &[GitSource]) -> String {
    let crates = CRATES_SOURCE_DIRECTORY.join("/");
    let mut body = if is_crates_io(index_url) {
        format!(
            "[source.crates-io]\nreplace-with = \"pnpm-crates-io\"\n\n[source.pnpm-crates-io]\ndirectory = \"{crates}\"\n",
        )
    } else {
        let source = toml::Value::from(pnpm_cargo_resolver::sparse_source(index_url));
        format!(
            "[source.crates-io]\nreplace-with = \"pnpm-registry\"\n\n[source.pnpm-registry]\nregistry = {source}\nreplace-with = \"pnpm-registry-directory\"\n\n[source.pnpm-registry-directory]\ndirectory = \"{crates}\"\n",
        )
    };
    if !git_sources.is_empty() {
        let git = GIT_SOURCE_DIRECTORY.join("/");
        let blocks = git_sources.iter().fold(String::new(), |mut blocks, source| {
            blocks.push('\n');
            blocks.push_str(&source.config_block());
            blocks
        });
        body = format!("{body}\n[source.{GIT_SOURCE_NAME}]\ndirectory = \"{git}\"\n{blocks}");
    }
    format!("{MANAGED_START}\n{body}{MANAGED_END}")
}

#[cfg(test)]
mod tests;

async fn prepare_workspace_slots<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    root_dir: &Path,
    cargo_lock_path: &Path,
    cargo_lock: &str,
    http_client: Arc<ThrottledClient>,
) -> Result<WorkspaceSlots> {
    let packages = parse_lockfile(cargo_lock, &config.cargo.index_url)
        .wrap_err_with(|| format!("parse {}", cargo_lock_path.display()))?;
    let git_sources = packages.git_sources();
    let logged_methods = Arc::new(AtomicU8::new(0));
    let store_dir = &config.store_dir;
    if !packages.crates.is_empty() || !packages.git.is_empty() {
        store_dir.init().into_diagnostic().wrap_err_with(|| {
            format!("initialize cargo package store at {}", store_dir.display())
        })?;
    }
    let crates = download_crates::<Reporter>(DownloadOptions {
        config,
        packages: packages.crates,
        http_client,
        logged_methods: Arc::clone(&logged_methods),
        requester: format!("cargo workspace at {}", root_dir.display()),
    })
    .await?;
    let git = git::vendor::<Reporter>(git::VendorOptions {
        packages: packages.git,
        store_dir,
        git_shallow_hosts: &config.git_shallow_hosts,
        package_import_method: config.package_import_method,
        logged_methods,
        concurrency: config.network_concurrency.clamp(1, 16),
        offline: config.offline,
    })
    .await?;

    Ok(WorkspaceSlots { crates, git, git_sources })
}

mod workspace_directory;

mod sparse_registry;

mod lockfile;

mod materialize;
