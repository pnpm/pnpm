//! Install the tools a git-hosted snapshot's `buildDependencies` record
//! (`lockBuildTools`), so the git fetcher can prepare the package with
//! exactly those.
//!
//! A tool is fetched like any lockfile entry, verified against its recorded
//! integrity, and imported into a private directory of its own: no install
//! links a build tool into `node_modules`.

use super::{
    InstallPackageBySnapshot, InstallPackageBySnapshotError, fetch::download_tarball,
    runtime::binary_variant_for_host,
};
use crate::{ImportIndexedDirError, ImportIndexedDirOpts, import_indexed_dir};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_git_fetcher::{BuildTools, LockedBuildTools, ProvisionedTool};
use pnpm_lockfile::{
    BuildToolArtifact, LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry,
    TarballResolution, build_tool_artifact,
};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{PrivateInstall, PrivateInstallError};
use pnpm_tarball::IngestTarballToStore;
use std::sync::Arc;
use std::{collections::HashMap, path::PathBuf};

/// The lockfile name of the runtime a package builds on. Every other tool
/// a snapshot records is the package manager that prepares it.
const NODE: &str = "node";

/// Why the build tools a git-hosted snapshot records could not be installed.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum BuildToolError {
    #[display(
        "The lockfile records {tool_key} as a build tool of {package_key}, but has no packages entry for it."
    )]
    #[diagnostic(
        code(ERR_PNPM_LOCKFILE_MISSING_BUILD_TOOL),
        help(r#"Run "pnpm update" for the dependency to record its build tools again."#)
    )]
    MissingEntry { package_key: String, tool_key: String },

    /// The record names something no build tool can be: an entry under
    /// another name, a second package manager, or a resolution that is not
    /// the tool's own artifact.
    #[display(
        "The lockfile records {tool_key} as a build tool of {package_key}, which is not a build tool pnpm can run."
    )]
    #[diagnostic(
        code(ERR_PNPM_LOCKFILE_INVALID_BUILD_TOOL),
        help(r#"Run "pnpm update" for the dependency to record its build tools again."#)
    )]
    InvalidRecord { package_key: String, tool_key: String },

    #[display("The lockfile records {tool_key} as a build tool, but its download is {installed}.")]
    #[diagnostic(
        code(ERR_PNPM_BUILD_TOOL_MISMATCH),
        help(
            "The lockfile entry of the tool may have been tampered with. Check it before you install."
        )
    )]
    NotTheRecordedTool { tool_key: String, installed: String },

    #[diagnostic(transparent)]
    PrivateInstall(#[error(source)] PrivateInstallError),

    #[display("Failed to install the build tool {tool_key}: {source}")]
    #[diagnostic(code(ERR_PNPM_BUILD_TOOL_INSTALL))]
    Import {
        tool_key: String,
        #[error(source)]
        source: ImportIndexedDirError,
    },

    #[display("Failed to read the manifest of the build tool {tool_key}: {source}")]
    #[diagnostic(code(ERR_PNPM_BUILD_TOOL_INSTALL))]
    ReadManifest {
        tool_key: String,
        #[error(source)]
        source: pnpm_package_manifest::PackageManifestError,
    },
}

struct InstalledBuildTool {
    tool: ProvisionedTool,
    _install: PrivateInstall,
}

type InstalledBuildToolSlot = Arc<tokio::sync::Mutex<Option<Arc<InstalledBuildTool>>>>;

#[derive(Default)]
pub struct BuildToolsCache {
    installed: tokio::sync::Mutex<HashMap<PackageKey, InstalledBuildToolSlot>>,
}

/// The installed tools of one package. The directories they live in go
/// when this is dropped.
pub(super) struct ProvisionedBuildTools {
    tools: LockedBuildTools,
}

/// What the git fetcher prepares one package with.
pub(super) enum PackageBuildTools {
    /// `lockBuildTools` is off.
    Detected,
    /// The recorded tools, installed. `None` when the lockfile records none
    /// or the package is not built.
    Locked(Option<Box<ProvisionedBuildTools>>),
}

impl PackageBuildTools {
    pub(super) fn identity(&self) -> Option<&str> {
        match self {
            PackageBuildTools::Locked(Some(provisioned)) => Some(&provisioned.tools.identity),
            PackageBuildTools::Detected | PackageBuildTools::Locked(None) => None,
        }
    }

    pub(super) fn as_build_tools(&self) -> BuildTools<'_> {
        match self {
            PackageBuildTools::Detected => BuildTools::Detected,
            PackageBuildTools::Locked(provisioned) => {
                BuildTools::Locked(provisioned.as_ref().map(|provisioned| &provisioned.tools))
            }
        }
    }
}

impl InstallPackageBySnapshot<'_> {
    /// The tools the git-hosted package of `snapshot` is prepared with,
    /// installed only when the prepare will run.
    pub(super) async fn build_tools_for<Reporter: self::Reporter>(
        &self,
        package_key: &PackageKey,
        snapshot: &SnapshotEntry,
        built: bool,
    ) -> Result<PackageBuildTools, InstallPackageBySnapshotError> {
        if !self.ctx.config.lock_build_tools {
            return Ok(PackageBuildTools::Detected);
        }
        if !built {
            return Ok(PackageBuildTools::Locked(None));
        }
        let provisioned = self.provision_build_tools::<Reporter>(package_key, snapshot).await?;
        Ok(PackageBuildTools::Locked(provisioned.map(Box::new)))
    }

    /// Install the tools `snapshot` records. `None` when it records none.
    async fn provision_build_tools<Reporter: self::Reporter>(
        &self,
        package_key: &PackageKey,
        snapshot: &SnapshotEntry,
    ) -> Result<Option<ProvisionedBuildTools>, InstallPackageBySnapshotError> {
        let Some(lockfile_identity) = pnpm_lockfile::build_tools_identity(snapshot, self.packages)
        else {
            return Ok(None);
        };
        let identity = crate::build_tools_identity(snapshot, self.packages)
            .unwrap_or_else(|| {
                format!("{lockfile_identity}|host-node-unavailable:{}", std::process::id())
            });
        let mut tools = LockedBuildTools { identity, ..LockedBuildTools::default() };
        for (alias, tool_key) in snapshot.build_dependency_keys() {
            let record = self.build_tool_record(package_key, alias, &tool_key)?;
            let slot = if alias.to_string() == NODE {
                &mut tools.node
            } else {
                &mut tools.package_manager
            };
            if slot.is_some() {
                return Err(invalid_record(package_key, &tool_key).into());
            }
            let tool_key = tool_key.without_peer();
            let install = {
                let mut installed = self.ctx.build_tools_cache.installed.lock().await;
                Arc::clone(
                    installed
                        .entry(tool_key.clone())
                        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(None))),
                )
            };
            let mut installed = install.lock().await;
            if installed.is_none() {
                let (tool, private_install) =
                    self.install_build_tool::<Reporter>(&tool_key, record).await?;
                *installed = Some(Arc::new(InstalledBuildTool { tool, _install: private_install }));
            }
            *slot = Some(
                installed
                    .as_ref()
                    .expect("tool was installed")
                    .tool
                    .clone(),
            );
        }
        Ok(Some(ProvisionedBuildTools { tools }))
    }

    /// The entry of the tool `alias` names, when it is one: recorded under
    /// its own name, and resolved to the artifact that tool can be.
    fn build_tool_record<'p>(
        &'p self,
        package_key: &PackageKey,
        alias: &pnpm_lockfile::PkgName,
        tool_key: &PackageKey,
    ) -> Result<(&'p PackageMetadata, BuildToolArtifact<'p>), BuildToolError> {
        let metadata = self.packages
            .and_then(|packages| packages.get(&tool_key.without_peer()))
            .ok_or_else(|| BuildToolError::MissingEntry {
                package_key: package_key.to_string(),
                tool_key: tool_key.to_string(),
            })?;
        let artifact = (tool_key.name == *alias)
            .then(|| build_tool_artifact(tool_key, &metadata.resolution))
            .flatten()
            .ok_or_else(|| invalid_record(package_key, tool_key))?;
        Ok((metadata, artifact))
    }

    async fn install_build_tool<Reporter: self::Reporter>(
        &self,
        tool_key: &PackageKey,
        (metadata, artifact): (&PackageMetadata, BuildToolArtifact<'_>),
    ) -> Result<(ProvisionedTool, PrivateInstall), InstallPackageBySnapshotError> {
        let cas_paths = self.fetch_build_tool::<Reporter>(tool_key, metadata, artifact).await?;
        let config = self.ctx.config;
        let (name, version) = (tool_key.name.to_string(), tool_key.suffix.version().to_string());
        let install = config.store_dir
            .create_private_install(&format!("{name}-{version}"))
            .map_err(BuildToolError::PrivateInstall)?;
        import_indexed_dir::<Reporter>(
            self.ctx.logged_methods,
            config.package_import_method,
            install.dir(),
            &cas_paths,
            ImportIndexedDirOpts::default(),
        )
        .map_err(|source| BuildToolError::Import { tool_key: tool_key.to_string(), source })?;
        check_installed_identity(tool_key, artifact, install.dir())?;
        let runs_on_node = matches!(artifact, BuildToolArtifact::Registry { .. });
        let tool = ProvisionedTool::from_dir(name, version, install.dir(), runs_on_node)
            .map_err(|source| BuildToolError::ReadManifest {
                tool_key: tool_key.to_string(),
                source,
            })?;
        Ok((tool, install))
    }

    /// The tool's files in the store, fetched and checked against the
    /// lockfile's integrity when the store does not have them yet.
    async fn fetch_build_tool<Reporter: self::Reporter>(
        &self,
        tool_key: &PackageKey,
        metadata: &PackageMetadata,
        artifact: BuildToolArtifact<'_>,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        match (artifact, &metadata.resolution) {
            (BuildToolArtifact::Registry { tarball, .. }, _) => {
                self.download_build_tool_tarball::<Reporter>(tool_key, metadata, tarball).await
            }
            (BuildToolArtifact::Archive, LockfileResolution::Variations(variations)) => {
                let binary =
                    binary_variant_for_host(variations, tool_key, self.runtime_platform_selector)?;
                self.fetch_binary::<Reporter>(binary, tool_key).await
            }
            (BuildToolArtifact::Archive, LockfileResolution::Binary(binary)) => {
                self.fetch_binary::<Reporter>(binary, tool_key).await
            }
            (BuildToolArtifact::Archive, _) => {
                unreachable!("build_tool_artifact admits only archive resolutions for an archive")
            }
        }
    }

    async fn download_build_tool_tarball<Reporter: self::Reporter>(
        &self,
        tool_key: &PackageKey,
        metadata: &PackageMetadata,
        tarball: &TarballResolution,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let package_id = tool_key.pkg_id();
        let ingest = self.ingest(metadata, &package_id);
        let download = IngestTarballToStore {
            package: pnpm_tarball::TarballPackage {
                integrity: tarball.integrity.as_ref(),
                url: &tarball.tarball,
                ..ingest.clone().package
            },
            ..ingest
        };
        download_tarball::<Reporter>(download, None, false).await
            .map_err(InstallPackageBySnapshotError::DownloadTarball)
    }
}

/// The installed manifest must name the package and version the record
/// stands for: a tarball of some other package does not become the tool by
/// matching the integrity the lockfile records for it.
fn check_installed_identity(
    tool_key: &PackageKey,
    artifact: BuildToolArtifact<'_>,
    dir: &std::path::Path,
) -> Result<(), BuildToolError> {
    let expected_name = match artifact {
        BuildToolArtifact::Registry { package, .. } => package.to_string(),
        BuildToolArtifact::Archive => tool_key.name.to_string(),
    };
    let expected_version = tool_key.suffix.version().to_string();
    let manifest = pnpm_package_manifest::safe_read_package_json_from_dir(dir)
        .map_err(|source| BuildToolError::ReadManifest { tool_key: tool_key.to_string(), source })?
        .unwrap_or_default();
    let field = |name: &str| {
        manifest
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
    };
    if field("name") == expected_name && field("version") == expected_version {
        return Ok(());
    }
    Err(BuildToolError::NotTheRecordedTool {
        tool_key: tool_key.to_string(),
        installed: format!("{}@{}", field("name"), field("version")),
    })
}

fn invalid_record(package_key: &PackageKey, tool_key: &PackageKey) -> BuildToolError {
    BuildToolError::InvalidRecord {
        package_key: package_key.to_string(),
        tool_key: tool_key.to_string(),
    }
}

impl From<BuildToolError> for InstallPackageBySnapshotError {
    fn from(error: BuildToolError) -> Self {
        InstallPackageBySnapshotError::BuildTool(error)
    }
}

#[cfg(test)]
mod tests;
