//! Installing an interpreter this machine does not have.
//!
//! The builds are [python-build-standalone]'s, the ones uv, rye, hatch
//! and mise install too. One release holds an interpreter of every
//! supported version line, and its `SHA256SUMS` names them all. With the
//! upstream releases, an exact patch pin may name a build from an older
//! release; those are found by release tag and their immutable checksum
//! files stay in pnpm's cache. A configured mirror remains self-contained
//! and supplies its own latest manifest, because the mirror contract has
//! no historical release index.
//!
//! [python-build-standalone]: https://github.com/astral-sh/python-build-standalone

mod releases;

use super::{InterpreterCommand, VersionRequest, command::interpreter_in};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_config::{Config, DEFAULT_PYTHON_DOWNLOAD_URL, RuntimeOnFail, Tool};
use pnpm_crypto_shasums_file::{ShasumsFileItem, fetch_moving_shasums_file_cached};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use std::{
    io::Write as _,
    path::{Path, PathBuf},
    time::Duration,
};

/// How long the release index is read from the cache before pnpm asks
/// whether a newer release has come out. The URL names the newest
/// release rather than a version, so what it answers changes.
const INDEX_MAX_AGE: Duration = Duration::from_hours(24);

const RELEASE_TAGS_URL: &str =
    "https://api.github.com/repos/astral-sh/python-build-standalone/tags";

/// A sha256 as the index carries it, which is what a row whose hash
/// pnpm could not read is not: an unreadable one parses to an empty
/// digest, and an empty digest is no build.
const SHA256_INTEGRITY_LEN: usize = "sha256-".len() + 44;

/// An interpreter is tens of megabytes, and a mirror serving something
/// else entirely is not read to the end to find that out.
const MAX_ARCHIVE_BYTES: usize = 256 * 1024 * 1024;

/// The interpreters pnpm can install, as one release's `SHA256SUMS` names
/// them.
pub(super) struct Releases {
    builds: Vec<Build>,
}

#[derive(Clone, Copy)]
pub(super) struct Source<'a> {
    releases_url: &'a str,
    tags_url: Option<&'a str>,
}

impl<'a> Source<'a> {
    pub(super) fn configured(config: &'a Config) -> Self {
        Self {
            releases_url: releases_url(config),
            tags_url: config
                .tool_mirror(Tool::Python)
                .is_none()
                .then_some(RELEASE_TAGS_URL),
        }
    }

    #[cfg(test)]
    pub(super) fn from_urls(releases_url: &'a str, tags_url: &'a str) -> Self {
        Self { releases_url, tags_url: Some(tags_url) }
    }
}

/// One interpreter a release offers: the file it is downloaded as, and
/// the digest that file has to have.
///
/// Every part is validated as it is read, because a release index is a
/// download naming what pnpm will run and where it will put it.
pub(super) struct Build {
    version: pep440_rs::Version,
    /// The `20260901` of `cpython-3.13.15+20260901-…`, which is the
    /// release the file belongs to.
    tag: String,
    /// What python-build-standalone calls the machine this build runs
    /// on, which pnpm knows before it reads any index.
    triple: String,
    file: String,
    integrity: ssri::Integrity,
}

/// Where the interpreter builds are downloaded from: the mirror
/// `tools.python.mirror` names, or python-build-standalone's own
/// releases. A mirror lays a release out the way that project does, so
/// only the host above it differs.
fn releases_url(config: &Config) -> &str {
    config.tool_mirror(Tool::Python).unwrap_or(DEFAULT_PYTHON_DOWNLOAD_URL)
}

impl Releases {
    pub(super) async fn read_from(
        config: &Config,
        client: &ThrottledClient,
        source: Source<'_>,
        exact: Option<&pep440_rs::Version>,
    ) -> Result<Self> {
        let Source { releases_url, tags_url } = source;
        let url = format!("{releases_url}/latest/download/SHA256SUMS");
        let index = fetch_moving_shasums_file_cached(
            client,
            &url,
            Some(&config.cache_dir),
            INDEX_MAX_AGE,
            config.retry_opts(),
        )
        .await
        .wrap_err_with(|| format!("read the Python interpreters {url} offers"))?;
        let mut builds = builds_in(&index);
        if let Some((exact, tags_url)) = exact
            .filter(|exact| !has_version(&builds, exact))
            .zip(tags_url)
            && let Some(build) =
                releases::historical_build(config, client, releases_url, tags_url, exact).await?
        {
            builds.push(build);
        }
        Ok(Self { builds })
    }

    /// The newest interpreter the project accepts, or `None` when no
    /// build the release offers is one.
    pub(super) fn best(
        &self,
        requires_python: Option<&pep440_rs::VersionSpecifiers>,
        request: Option<&VersionRequest>,
    ) -> Option<&Build> {
        self.builds
            .iter()
            .filter(|build| {
                requires_python.is_none_or(|specifiers| specifiers.contains(&build.version))
                    && request.is_none_or(|request| request.accepts(&build.version))
            })
            .max_by_key(|build| &build.version)
    }
}

pub(super) fn exact_version(
    requires_python: Option<&pep440_rs::VersionSpecifiers>,
    request: Option<&VersionRequest>,
) -> Option<pep440_rs::Version> {
    let requested = request
        .filter(|request| request.release.len() >= 3)
        .map(|request| pep440_rs::Version::new(request.release.iter().copied()));
    let required = requires_python.and_then(|specifiers| {
        specifiers
            .iter()
            .find(|specifier| {
                matches!(
                    specifier.operator(),
                    pep440_rs::Operator::Equal | pep440_rs::Operator::ExactEqual,
                )
            })
            .map(|specifier| specifier.version().clone())
    });
    required.or_else(|| {
        requested.filter(|version| {
            requires_python.is_none_or(|specifiers| specifiers.contains(version))
        })
    })
}

fn has_version(builds: &[Build], exact: &pep440_rs::Version) -> bool {
    builds
        .iter()
        .any(|build| build.version == *exact)
}

impl Build {
    pub(super) fn version(&self) -> &pep440_rs::Version {
        &self.version
    }

    /// Where this build is installed, which is where an earlier install
    /// of it already put it. The name is the one pnpm read the build as,
    /// so no index can name a directory of its own.
    fn directory(&self, config: &Config) -> PathBuf {
        config.store_dir
            .root()
            .join("python")
            .join(format!("cpython-{}+{}-{}", self.version, self.tag, self.triple))
    }

    /// A build already installed answers without a download.
    pub(super) async fn install(
        &self,
        config: &Config,
        client: &ThrottledClient,
    ) -> Result<InterpreterCommand> {
        let directory = self.directory(config);
        if !directory.is_dir() {
            let archive = self.download(config, client).await?;
            unpack(archive.path(), &directory)?;
        }
        let executable = interpreter_in(&directory);
        let Some(executable) = executable.to_str().filter(|_| executable.is_file()) else {
            bail!("the Python interpreter pnpm installed has no {}", executable.display());
        };
        Ok(InterpreterCommand::program(executable))
    }

    /// Refuses an archive whose bytes are not what the release says they
    /// are, before anything is unpacked from it.
    async fn download(
        &self,
        config: &Config,
        client: &ThrottledClient,
    ) -> Result<tempfile::NamedTempFile> {
        let url = format!("{}/download/{}/{}", releases_url(config), self.tag, self.file);
        let response = client
            .get_limited_bytes_with_secure_auth_and_retry(
                &url,
                &AuthHeaders::default(),
                None,
                config.retry_opts(),
                MAX_ARCHIVE_BYTES,
            )
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("download the Python interpreter {url}"))?;
        if response.body_truncated {
            bail!("the Python interpreter at {url} exceeds {MAX_ARCHIVE_BYTES} bytes");
        }
        if !response.status.is_success() {
            bail!("downloading the Python interpreter {url} returned {}", response.status);
        }
        let mut checker = ssri::IntegrityChecker::new(self.integrity.clone());
        checker.input(&response.body);
        if checker.result().is_err() {
            bail!(
                "the Python interpreter downloaded from {url} is not the one the release names, \
                 which is the one hashing to {}",
                self.integrity,
            );
        }
        let directory = config.store_dir.tmp();
        std::fs::create_dir_all(&directory).into_diagnostic()?;
        let mut file = tempfile::NamedTempFile::new_in(&directory).into_diagnostic()?;
        file.write_all(&response.body).into_diagnostic()?;
        Ok(file)
    }
}

/// What an install says when it installs a version other than the one a
/// `.python-version` file asks for.
pub(super) fn report_unmet_request<Reporter: self::Reporter + 'static>(
    releases: &Releases,
    root: &Path,
    request: &VersionRequest,
    installing: &pep440_rs::Version,
) {
    let unmet = if releases
        .best(None, Some(request))
        .is_some()
    {
        "which this project's requires-python does not accept"
    } else {
        "which is not published for this machine"
    };
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: format!(
            "Installing Python {installing} for {}: {} asks for Python {}, {unmet}",
            root.display(),
            request.file.display(),
            request.version(),
        ),
    }));
}

pub(super) fn allowed(config: &Config) -> bool {
    refused(config).is_none()
}

/// Why this run installs no interpreter, for the install that needed
/// one. An offline install has nowhere to download from, and every
/// `runtimeOnFail` mode but `download` asks pnpm to report an unmet
/// runtime rather than install it.
pub(super) fn refused(config: &Config) -> Option<&'static str> {
    if config.offline {
        return Some("the install is offline");
    }
    match config.runtime_on_fail {
        None | Some(RuntimeOnFail::Download) => None,
        Some(RuntimeOnFail::Error) => Some("runtimeOnFail is error"),
        Some(RuntimeOnFail::Warn) => Some("runtimeOnFail is warn"),
        Some(RuntimeOnFail::Ignore) => Some("runtimeOnFail is ignore"),
    }
}

/// The builds of the index that install on this machine: the ordinary
/// interpreter of one platform, without the variants a project asks for
/// by name rather than by version.
fn builds_in(index: &[ShasumsFileItem]) -> Vec<Build> {
    let Some(triple) = host_triple() else { return Vec::new() };
    let suffix = format!("-{triple}-install_only_stripped.tar.gz");
    index
        .iter()
        .filter_map(|item| read_build(item, &triple, &suffix))
        .collect()
}

/// One row of the index, as the build it names, or nothing when it names
/// something else or names it in a way pnpm will not use as a file name:
/// the index decides what pnpm downloads and where it puts it, so every
/// part of it is checked rather than trusted.
fn read_build(item: &ShasumsFileItem, triple: &str, suffix: &str) -> Option<Build> {
    if item.integrity.len() != SHA256_INTEGRITY_LEN {
        return None;
    }
    let named = item.file_name.strip_prefix("cpython-")?.strip_suffix(suffix)?;
    let (version, tag) = named.split_once('+')?;
    // A release is named by the day it was built, which is also what
    // keeps the name pnpm installs the build under a name and not a path.
    if tag.is_empty() || !tag.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    Some(Build {
        version: version.parse().ok()?,
        tag: tag.to_string(),
        triple: triple.to_string(),
        file: item.file_name.clone(),
        integrity: item.integrity.parse().ok()?,
    })
}

/// What python-build-standalone calls the interpreter of this machine.
/// `None` where it builds none, which is where pnpm installs none.
pub(super) fn host_triple() -> Option<String> {
    let architecture = std::env::consts::ARCH;
    Some(match std::env::consts::OS {
        "linux" => {
            let architecture = match architecture {
                "x86_64" | "aarch64" | "riscv64" | "s390x" => architecture,
                // Only the little-endian PowerPC is built: `powerpc64` is
                // Rust's name for the big-endian one.
                "powerpc64le" => "ppc64le",
                _ => return None,
            };
            let libc = match pnpm_detect_libc::detect() {
                Some(pnpm_detect_libc::Implementation::Musl) => "musl",
                _ => "gnu",
            };
            format!("{architecture}-unknown-linux-{libc}")
        }
        "macos" => match architecture {
            "x86_64" | "aarch64" => format!("{architecture}-apple-darwin"),
            _ => return None,
        },
        "windows" => match architecture {
            "x86_64" | "aarch64" => format!("{architecture}-pc-windows-msvc"),
            "x86" => "i686-pc-windows-msvc".to_string(),
            _ => return None,
        },
        _ => return None,
    })
}

/// Beside where it belongs and moved there, so that a directory under
/// `python` is one an install can use and never a download that stopped
/// halfway.
fn unpack(archive: &Path, directory: &Path) -> Result<()> {
    within(archive, INTERPRETER_BOUNDS)?;
    let parent = directory.parent().expect("an installed interpreter has a parent directory");
    std::fs::create_dir_all(parent).into_diagnostic()?;
    let staged = tempfile::TempDir::new_in(parent).into_diagnostic()?;
    let mut unpacked = tar::Archive::new(reader(archive, INTERPRETER_BOUNDS)?);
    unpacked.set_preserve_permissions(true);
    unpacked
        .unpack(staged.path())
        .into_diagnostic()
        .wrap_err("unpack the Python interpreter pnpm downloaded")?;
    match std::fs::rename(staged.keep(), directory) {
        Ok(()) => Ok(()),
        // Another install of the same build won the race, which unpacked
        // the same interpreter this one did.
        Err(_) if directory.is_dir() => Ok(()),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("install the Python interpreter into {}", directory.display())
            }),
    }
}

/// What an archive may hold to be an interpreter, in the two costs that
/// its compressed size does not bound: a gzip archive of a few hundred
/// megabytes holds hundreds of gigabytes, or a million empty files, and
/// a file costs an inode whatever it holds.
#[derive(Clone, Copy)]
struct Bounds {
    bytes: u64,
    entries: usize,
}

/// An interpreter unpacks to a little over a hundred megabytes in a few
/// thousand files. Both leave the room a build may still grow into.
const INTERPRETER_BOUNDS: Bounds = Bounds { bytes: 512 * 1024 * 1024, entries: 50_000 };

/// Whether the archive is one, read through to the end before anything
/// is written, so an archive that is not stops the install rather than
/// leaving a partial unpacking to be cleaned up.
fn within(archive: &Path, bounds: Bounds) -> Result<()> {
    let mut read = tar::Archive::new(reader(archive, bounds)?);
    let entries = read
        .entries()
        .into_diagnostic()
        .wrap_err(READ)?;
    for (count, entry) in entries.enumerate() {
        entry.into_diagnostic().wrap_err(READ)?;
        if count >= bounds.entries {
            bail!("{READ}: it holds more than {} entries", bounds.entries);
        }
    }
    Ok(())
}

const READ: &str = "read the Python interpreter pnpm downloaded";

fn reader(
    archive: &Path,
    bounds: Bounds,
) -> Result<Bounded<flate2::read::GzDecoder<std::fs::File>>> {
    let file = std::fs::File::open(archive).into_diagnostic().wrap_err(READ)?;
    Ok(Bounded { inner: flate2::read::GzDecoder::new(file), left: bounds.bytes })
}

/// A stream that ends in an error once it has given out more than it was
/// allowed to.
struct Bounded<Stream> {
    inner: Stream,
    left: u64,
}

impl<Stream: std::io::Read> std::io::Read for Bounded<Stream> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.left = self.left
            .checked_sub(read as u64)
            .ok_or_else(|| std::io::Error::other("it unpacks to more than it may"))?;
        Ok(read)
    }
}

#[cfg(test)]
mod tests;
