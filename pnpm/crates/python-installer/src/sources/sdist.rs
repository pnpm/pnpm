//! Building the wheel a release's source distribution produces.
//!
//! A release that publishes no wheel this interpreter installs is
//! installed by building the source distribution the index serves beside
//! it, which is also where its requirements are read from: an sdist
//! declares them only in the wheel it produces.

use super::{MAX_WHEEL_BYTES, read_manifest};
use crate::{
    build::{self, Buildable, Contract},
    host,
    registry::Registry,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::Version;
use pep508_rs::PackageName;
use pnpm_python_resolver::{LockedSdist, parse_requirement};
use pnpm_reporter::Reporter;
use pnpm_tarball::{ArchiveStoreProjection, IngestTarballToStore, IngestZipArchiveToStore};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

impl Registry<'_> {
    /// Build the release's source distribution, leaving the wheel it
    /// produced where a locked wheel of the same release would be: read
    /// for its requirements while the project resolves, and installed
    /// into its environment afterwards.
    pub(super) async fn build_sdist<Reporter: self::Reporter + 'static>(
        &mut self,
        name: &PackageName,
        version: &Version,
    ) -> Result<()> {
        let sdist = self.resolution.packages.candidates
            .get(name)
            .and_then(|versions| versions.get(version))
            .and_then(pnpm_python_resolver::Candidate::sdist)
            .ok_or_else(|| {
                miette::miette!("Python package {name} {version} is not a source distribution")
            })?
            .clone();
        self.check_buildable(name, version, &sdist)?;
        let files = self.fetch_sdist::<Reporter>(&sdist).await?;
        let source = tokio::task::spawn_blocking(move || unpack(&files)).await
            .into_diagnostic()?
            .wrap_err_with(|| format!("unpack the Python source distribution {}", sdist.name))?;
        let manifest = read_manifest(source.path()).await?;
        let built = Box::pin(self.sources.prepare.build::<Reporter>(Buildable {
            root: source.path(),
            manifest: &manifest,
            editable: false,
            contract: Contract::Interpreter,
        }))
        .await?;
        let build::Build::Made(mut built) = built else {
            bail!(
                "the build requirements of the Python source distribution of {name} are not \
                 approved under allowBuilds",
            );
        };
        self.check_source_wheel(&built.wheel, name, version)?;
        check_identity(&built.wheel.metadata, name, version, &sdist)?;
        // An index release is not a direct URL requirement, so PEP 610
        // has the installer record nothing about where it came from.
        built.wheel.direct_url = None;
        self.remember(name.clone(), version.clone(), built.wheel.clone());
        self.sources.built.insert((name.clone(), version.clone()), *built);
        Ok(())
    }

    /// Refuse a source distribution pnpm will not build: one nothing has
    /// approved to run, and one a project needs for an environment other
    /// than the one running the install.
    fn check_buildable(
        &self,
        name: &PackageName,
        version: &Version,
        sdist: &LockedSdist,
    ) -> Result<()> {
        if self.resolution.target != self.interpreter.target {
            bail!(
                "the Python release {name}=={version} publishes no wheel for every environment \
                 this project locks for, and pnpm builds {} only for the interpreter running the \
                 install",
                sdist.name,
            );
        }
        let approval = parse_requirement(name.as_ref())?;
        if !build::unapproved(self.config, &[approval]).is_empty() {
            bail!(
                "{name}=={version} publishes no wheel this interpreter installs, so pnpm would \
                 build {} to install it. That runs the release's own build backend, which \
                 requires approval of pkg:pypi/{name} under allowBuilds.",
                sdist.name,
            );
        }
        Ok(())
    }

    /// The archive's files in the store, verified against the digest the
    /// index published, which is also where a later install and an
    /// offline one read them from.
    async fn fetch_sdist<Reporter: self::Reporter + 'static>(
        &self,
        sdist: &LockedSdist,
    ) -> Result<HashMap<String, PathBuf>> {
        let integrity = sdist.integrity()?;
        let package_id = format!("python:{}", sdist.name);
        let fetching = pnpm_tarball::ArchiveFetchOptions {
            http_client: self.client,
            auth_headers: &self.index.auth,
            retry_opts: self.config.retry_opts(),
            offline: self.config.offline,
        };
        if sdist.is_zip() {
            return IngestZipArchiveToStore {
                fetching,
                package: pnpm_tarball::ZipArchivePackage {
                    max_bytes: Some(MAX_WHEEL_BYTES),
                    integrity: &integrity,
                    url: &sdist.url,
                    id: &package_id,
                },
                store: self.store.clone(),
                requester: "Python environment",
                archive_prefix: Some(sdist.root()),
                ignore_file_pattern: None,
                store_projection: ArchiveStoreProjection::RawArchive,
            }
            .run_without_mem_cache::<Reporter>()
            .await
            .into_diagnostic();
        }
        // A tar archive of a source distribution holds one directory
        // named after the release, which the extractor strips the way it
        // strips an npm tarball's own.
        IngestTarballToStore {
            fetching,
            package: pnpm_tarball::TarballPackage {
                integrity: Some(&integrity),
                unpacked_size: None,
                file_count: None,
                url: &sdist.url,
                id: &package_id,
            },
            store: self.store.clone(),
            requester: "Python environment",
            ignore_file_pattern: None,
            progress_reported: None,
            store_projection: ArchiveStoreProjection::RawArchive,
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .into_diagnostic()
    }
}

/// The source tree a build runs in: the archive's files copied out of the
/// store, so a backend that writes beside them is not writing into
/// content every project on this machine shares.
fn unpack(files: &HashMap<String, PathBuf>) -> Result<tempfile::TempDir> {
    let source = tempfile::tempdir().into_diagnostic()?;
    for (entry, stored) in files {
        let path = entry_path(source.path(), entry)?;
        std::fs::create_dir_all(path.parent().expect("an entry path has a parent"))
            .into_diagnostic()?;
        std::fs::write(&path, std::fs::read(stored).into_diagnostic()?).into_diagnostic()?;
        pnpm_fs::file_mode::restore_exec_bit_from_cas_suffix(stored, &path).into_diagnostic()?;
    }
    Ok(source)
}

fn entry_path(source: &Path, entry: &str) -> Result<PathBuf> {
    let relative = Path::new(entry);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("unsafe path in a Python source distribution: {entry}");
    }
    Ok(source.join(relative))
}

/// Refuse a wheel that is not the release the index said the archive
/// carries. The lockfile pins the archive by name and digest, so what it
/// builds has to be what the lockfile describes.
fn check_identity(
    metadata: &host::WheelMetadata,
    name: &PackageName,
    version: &Version,
    sdist: &LockedSdist,
) -> Result<()> {
    if metadata.name.parse::<PackageName>().into_diagnostic()? != *name
        || metadata.version.parse::<Version>().into_diagnostic()? != *version
    {
        bail!(
            "the Python source distribution {} built {} {}, not {name} {version}",
            sdist.name,
            metadata.name,
            metadata.version,
        );
    }
    Ok(())
}
