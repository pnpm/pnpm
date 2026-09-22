//! Building the wheel a release's source distribution produces.
//!
//! A release that publishes no wheel this interpreter installs is
//! installed by building the source distribution the index serves beside
//! it, which is also where its requirements are read from: an sdist
//! declares them only in the wheel it produces.

use super::{
    MAX_WHEEL_BYTES,
    read_manifest,
};
use crate::{
    build::{
        self,
        Buildable,
        Contract,
    },
    host,
    registry::Registry,
};
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
    bail,
};
use pep440_rs::Version;
use pep508_rs::PackageName;
use pnpm_python_resolver::{
    LockedSdist,
    parse_requirement,
};
use pnpm_reporter::Reporter;
use pnpm_tarball::{
    ArchiveStoreProjection,
    IngestTarballToStore,
    IngestZipArchiveToStore,
};
use std::{
    collections::HashMap,
    path::{
        Component,
        Path,
        PathBuf,
    },
};

/// What the store's progress reporting calls a Python download, the
/// same for every artifact a Python install fetches.
const REQUESTER: &str = "Python environment";

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
        let root = sdist.root().to_owned();
        let source = tokio::task::spawn_blocking(move || unpack(&files, &root))
            .await
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
        let mut built = match built {
            build::Build::Made(built) => built,
            build::Build::NotApproved(names) => bail!(
                "building the Python source distribution of {name} needs approved build \
                 requirements: {}. Add them under allowBuilds in pnpm-workspace.yaml.",
                names.join(", "),
            ),
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

    /// Refuse a source distribution pnpm will not build: one served from
    /// somewhere a Python artifact may not be fetched from, one nothing
    /// has approved to run, and one only an environment other than the
    /// running one asked for.
    ///
    /// That last case is what is left of the interpreter restriction
    /// once a release's requirements are read once per version, as they
    /// are for wheels: where the running interpreter reaches the release
    /// too, its build answers for every environment, and a replay
    /// elsewhere builds the archive again and is re-solved against what
    /// that build declares.
    fn check_buildable(
        &self,
        name: &PackageName,
        version: &Version,
        sdist: &LockedSdist,
    ) -> Result<()> {
        // The store fetches a `file:` URL from the filesystem, so the
        // scheme decides whether a build reads this machine rather than
        // the index.
        pnpm_python_resolver::validate_url(&sdist.url.parse().into_diagnostic()?)?;
        if self.resolution.target != self.interpreter.target {
            bail!(
                "only an environment other than the running one needs {name}=={version}, which \
                 publishes no wheel it installs, and pnpm builds {} only with the interpreter \
                 running the install",
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
            return self.fetch_source_zip::<Reporter>(sdist, fetching, &integrity, &package_id)
                .await;
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
            requester: REQUESTER,
            ignore_file_pattern: None,
            progress_reported: None,
            store_projection: ArchiveStoreProjection::RawArchive,
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .into_diagnostic()
    }

    /// The zip container, whose extractor is told the directory to strip
    /// rather than stripping whatever comes first.
    async fn fetch_source_zip<Reporter: self::Reporter + 'static>(
        &self,
        sdist: &LockedSdist,
        fetching: pnpm_tarball::ArchiveFetchOptions<'_>,
        integrity: &ssri::Integrity,
        package_id: &str,
    ) -> Result<HashMap<String, PathBuf>> {
        IngestZipArchiveToStore {
            fetching,
            package: pnpm_tarball::ZipArchivePackage {
                max_bytes: Some(MAX_WHEEL_BYTES),
                integrity,
                url: &sdist.url,
                id: package_id,
            },
            store: self.store.clone(),
            requester: REQUESTER,
            archive_prefix: Some(sdist.root()),
            ignore_file_pattern: None,
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
///
/// Each file is streamed rather than read whole, so the memory this
/// holds does not follow the size of whatever an index served. The
/// destination is created fresh, which is what leaves the tree writable:
/// a store blob is read-only, and a copy that carried its mode over
/// would hand the backend a source tree it cannot build in.
fn unpack(files: &HashMap<String, PathBuf>, root: &str) -> Result<tempfile::TempDir> {
    let source = tempfile::tempdir().into_diagnostic()?;
    let nested = nested_in_its_release_directory(files, root);
    for (entry, stored) in files {
        let path = entry_path(source.path(), strip_release_directory(entry, root, nested))?;
        std::fs::create_dir_all(path.parent().expect("an entry path has a parent"))
            .into_diagnostic()?;
        let mut blob = std::fs::File::open(stored)
            .into_diagnostic()
            .wrap_err_with(|| format!("read the stored Python source file {entry}"))?;
        let mut file = std::fs::File::create(&path).into_diagnostic()?;
        std::io::copy(&mut blob, &mut file).into_diagnostic()?;
        pnpm_fs::file_mode::restore_exec_bit_from_cas_suffix(stored, &path).into_diagnostic()?;
    }
    Ok(source)
}

/// Whether the extractor left the release directory on every entry.
///
/// It strips one leading component, which an entry written with the `./`
/// prefix GNU tar emits spends on that prefix instead of on the release
/// directory. A zip whose entries carry that prefix misses its own strip
/// for the same reason. Both shapes have to end up rooted at the
/// manifest, or the backend runs in a directory that has none.
///
/// The release directory is named after the archive, so an archive whose
/// entries say otherwise is left exactly as the extractor produced it.
fn nested_in_its_release_directory(files: &HashMap<String, PathBuf>, root: &str) -> bool {
    !files.is_empty()
        && files
            .keys()
            .all(|entry| {
                entry
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('/'))
            })
}

fn strip_release_directory<'a>(entry: &'a str, root: &str, nested: bool) -> &'a str {
    if nested { &entry[root.len() + 1..] } else { entry }
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
