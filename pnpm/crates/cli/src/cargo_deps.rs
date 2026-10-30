pub(crate) mod add;
pub(crate) mod toolchain;

pub(crate) use lockfile::{canonical_cargo_path, workspace_metadata, workspace_root};
pub(crate) use sparse_registry::{
    cargo_auth_headers, fetch_crate_index_file, latest_version, resolve_target_version,
};
pub(crate) use toolchain::{installs_toolchains, provision as provision_toolchain};

use crate::{
    cargo_deps::git::{GIT_SOURCE_DIRECTORY, GIT_SOURCE_NAME, GitPackage, GitSource},
    ecosystem_install::{EcosystemManifest, EcosystemWorkspaceInventory, InstallContext},
    process::Command,
};
use cargo_util_schemas::index::RegistryConfig;
use futures_util::{StreamExt, TryStreamExt, stream};

use lockfile::{
    LockedCrate, discover_workspace_roots, parse_lockfile, read_or_resolve_lockfile,
    validate_package_field, verify_existing_lockfile,
};
use materialize::{DownloadOptions, add_cargo_checksum, download_crates, slot_is_complete};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_cargo_resolver::is_crates_io;
use pnpm_config::Config;
use pnpm_deps_restorer::{CARGO_CHECKSUM_FILE, ImportIndexedDirOpts, import_indexed_dir};
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
    str::FromStr,
    sync::{Arc, atomic::AtomicU8},
};
use workspace_directory::{
    ManagedDirectory, ensure_workspace_directory, force_workspace_symlink, read_workspace_file,
    write_workspace_file,
};

use sources::{link_workspace, write_cargo_config, write_sources_config};

mod build_std;
mod checksum_cache;
mod git;
mod registry_auth;
mod resolution;
mod sources;

const WORKSPACE_INSTALL_CONCURRENCY: usize = 8;

/// Directory the registry crates are linked into, relative to the Cargo
/// workspace root.
const CRATES_SOURCE_DIRECTORY: [&str; 3] = [".pnpm", "crates", "crates-io"];
/// Directory, relative to the Cargo workspace root, of [`SOURCES_CONFIG`].
const SOURCES_CONFIG_DIRECTORY: [&str; 2] = [".pnpm", "crates"];
/// The source replacement for the vendored crates. `.cargo/config.toml`
/// includes it as optional, so a checkout without `.pnpm` still builds with
/// plain Cargo.
const SOURCES_CONFIG: &str = "config.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoLockfilePolicy {
    UseExisting,
    Resolve,
}

#[derive(Deserialize)]
pub(crate) struct CargoWorkspaceMetadata {
    pub(crate) workspace_root: PathBuf,
    pub(crate) packages: Vec<CargoWorkspacePackage>,
}

#[derive(Deserialize)]
pub(crate) struct CargoWorkspacePackage {
    pub(crate) manifest_path: PathBuf,
    #[serde(default)]
    pub(crate) dependencies: Vec<CargoWorkspaceDependency>,
}

/// A dependency declaration as `cargo metadata` reports it, with the
/// features of the workspace entry it inherits from merged in. Each
/// target-specific table declaring it is an entry of its own.
#[derive(Deserialize)]
pub(crate) struct CargoWorkspaceDependency {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) rename: Option<String>,
    pub(crate) req: String,
    #[serde(default)]
    pub(crate) features: Vec<String>,
}

pub(crate) async fn plan<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    inventory: &EcosystemWorkspaceInventory,
) -> Result<InstallTask<'static>> {
    let manifests = inventory.manifests(EcosystemManifest::Cargo).await?;
    let checkout = checkout(inventory.workspace_root());
    toolchain::provision::<Reporter>(
        context.config,
        &context.http_client,
        manifests,
        checkout.as_deref(),
    )
    .await?;
    let workspaces = discover_workspace_roots(manifests, checkout.as_deref()).await?;
    if context.frozen_lockfile {
        workspaces.iter().try_for_each(verify_existing_lockfile)?;
    }
    let roots = workspaces
        .into_iter()
        .map(|workspace| workspace.root)
        .collect::<Vec<_>>();
    let metadata = roots
        .iter()
        .flat_map(|root| metadata_paths(root))
        .collect();
    Ok(InstallTask::new(
        metadata,
        prepare::<Reporter>(context, roots, checkout, CargoLockfilePolicy::UseExisting),
    ))
}

/// The directory the repository controls, resolved for comparison against
/// the canonical Cargo workspace roots. A Cargo workspace may sit inside it
/// rather than at it, so everything at or under it is checkout content and
/// what lies above it is the machine's.
///
/// [`None`] when the path does not resolve, which counts every configuration
/// file in scope as checkout content: a boundary pnpm cannot compare must
/// not widen what a checkout can point it at.
pub(crate) fn checkout(dir: &Path) -> Option<PathBuf> {
    dunce::canonicalize(dir).ok()
}

pub(crate) fn metadata_paths(root: &Path) -> [PathBuf; 3] {
    [
        root.join("Cargo.lock"),
        root.join(".cargo/config.toml"),
        root.join(SOURCES_CONFIG_DIRECTORY.join("/"))
            .join(SOURCES_CONFIG),
    ]
}

pub(crate) async fn prepare<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    roots: Vec<PathBuf>,
    checkout: Option<PathBuf>,
    lockfile_policy: CargoLockfilePolicy,
) -> Result<Vec<Prepared>> {
    let InstallContext {
        config,
        http_client,
        lockfile_only,
        frozen_lockfile,
    } = context;
    let checkout = checkout.as_deref();
    let mut prepared = stream::iter(roots)
        .map(|root| {
            let http_client = Arc::clone(&http_client);
            async move {
                prepare_workspace::<Reporter>(
                    config,
                    &root,
                    checkout,
                    lockfile_only,
                    frozen_lockfile,
                    lockfile_policy,
                    http_client,
                )
                .await
            }
        })
        .buffer_unordered(WORKSPACE_INSTALL_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    prepared.sort_by(|left, right| left.root.cmp(&right.root));
    Ok(prepared)
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
            write_sources_config(&self.root, &self.index_url, &slots.git_sources)?;
            write_cargo_config(&self.root)?;
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
    checkout: Option<&Path>,
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
        checkout,
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
            index_url: config.cargo_index_url().to_string(),
        });
    }
    let slots = prepare_workspace_slots::<Reporter>(
        config,
        root_dir,
        checkout,
        &cargo_lock_path,
        &cargo_lock,
        http_client,
    )
    .await?;

    Ok(Prepared {
        root: root_dir.to_path_buf(),
        lock: cargo_lock,
        slots: Some(slots),
        index_url: config.cargo_index_url().to_string(),
    })
}

#[cfg(test)]
mod tests;

async fn prepare_workspace_slots<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    root_dir: &Path,
    checkout: Option<&Path>,
    cargo_lock_path: &Path,
    cargo_lock: &str,
    http_client: Arc<ThrottledClient>,
) -> Result<WorkspaceSlots> {
    let packages = parse_lockfile(cargo_lock, config.cargo_index_url())
        .wrap_err_with(|| format!("parse {}", cargo_lock_path.display()))?;
    let packages = build_std::include_packages(root_dir, checkout, packages).await?;
    let git_sources = packages.git_sources();
    let logged_methods = Arc::new(AtomicU8::new(0));
    let store_dir = &config.store_dir;
    if !packages.crates.is_empty() || !packages.git.is_empty() {
        store_dir
            .init()
            .into_diagnostic()
            .wrap_err_with(|| {
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
