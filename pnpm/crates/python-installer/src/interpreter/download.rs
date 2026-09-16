//! Installing an interpreter this machine does not have.
//!
//! The builds are [python-build-standalone]'s, the ones uv, rye, hatch
//! and mise install too. One release holds an interpreter of every
//! supported version line, and its `SHA256SUMS` names them all, so
//! choosing a build and verifying what was downloaded read one file.
//!
//! [python-build-standalone]: https://github.com/astral-sh/python-build-standalone

use super::{InterpreterCommand, VersionRequest, command::interpreter_in};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_config::{Config, PythonDownloads};
use pnpm_network::{AuthHeaders, ThrottledClient};
use std::{
    io::Write as _,
    path::{Path, PathBuf},
    time::Duration,
};

/// How long the release index is read from the cache before pnpm asks
/// whether a newer release has come out.
const INDEX_MAX_AGE: Duration = Duration::from_hours(24);

/// A release names one interpreter per version line for every platform it
/// builds for, so the index stays far below this.
const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;

/// An interpreter is tens of megabytes, and a mirror serving something
/// else entirely is not read to the end to find that out.
const MAX_ARCHIVE_BYTES: usize = 256 * 1024 * 1024;

/// The interpreters pnpm can install, as one release's `SHA256SUMS` names
/// them.
pub(super) struct Releases {
    builds: Vec<Build>,
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
    sha256: String,
}

impl Releases {
    /// The interpreters pnpm can install.
    pub(super) async fn read(config: &Config, client: &ThrottledClient) -> Result<Self> {
        let cache = config.cache_dir.join("python-runtimes").join("SHA256SUMS");
        let index = match cached_index(&cache) {
            Some(index) => index,
            None => download_index(config, client, &cache).await?,
        };
        Ok(Self { builds: builds_in(&index) })
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
        let url = format!("{}/download/{}/{}", config.python.download_url, self.tag, self.file);
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
        let digest = pnpm_crypto_hash::create_hex_hash_bytes(&response.body);
        if digest != self.sha256 {
            bail!(
                "the Python interpreter downloaded from {url} is not the one the release names: \
                 its sha256 is {digest}, not {}",
                self.sha256,
            );
        }
        let directory = config.store_dir.tmp();
        std::fs::create_dir_all(&directory).into_diagnostic()?;
        let mut file = tempfile::NamedTempFile::new_in(&directory).into_diagnostic()?;
        file.write_all(&response.body).into_diagnostic()?;
        Ok(file)
    }
}

pub(super) fn allowed(config: &Config) -> bool {
    refused(config).is_none()
}

/// Why this run installs no interpreter, for the install that needed
/// one. An offline install has nowhere to download from, and a workspace
/// can turn downloads off.
pub(super) fn refused(config: &Config) -> Option<&'static str> {
    if config.offline {
        return Some("the install is offline");
    }
    (config.python.downloads != PythonDownloads::Auto).then_some("python.downloads is never")
}

/// The cached index, while it is recent enough to still name what the
/// release holds.
fn cached_index(cache: &Path) -> Option<String> {
    let age = cache
        .metadata()
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?;
    (age < INDEX_MAX_AGE)
        .then(|| std::fs::read_to_string(cache).ok())
        .flatten()
}

async fn download_index(config: &Config, client: &ThrottledClient, cache: &Path) -> Result<String> {
    let url = format!("{}/latest/download/SHA256SUMS", config.python.download_url);
    let response = client
        .get_limited_bytes_with_secure_auth_and_retry(
            &url,
            &AuthHeaders::default(),
            None,
            config.retry_opts(),
            MAX_INDEX_BYTES,
        )
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("read the Python interpreters {url} offers"))?;
    if response.body_truncated {
        bail!("the Python interpreter index at {url} exceeds {MAX_INDEX_BYTES} bytes");
    }
    if !response.status.is_success() {
        bail!("reading the Python interpreter index {url} returned {}", response.status);
    }
    let index = String::from_utf8(response.body)
        .into_diagnostic()
        .wrap_err_with(|| format!("read the Python interpreters {url} offers"))?;
    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent).into_diagnostic()?;
    }
    pnpm_fs::write_atomic(cache, index.as_bytes()).into_diagnostic()?;
    Ok(index)
}

/// The builds of the index that install on this machine: the ordinary
/// interpreter of one platform, without the variants a project asks for
/// by name rather than by version.
fn builds_in(index: &str) -> Vec<Build> {
    let Some(triple) = host_triple() else { return Vec::new() };
    let suffix = format!("-{triple}-install_only_stripped.tar.gz");
    index
        .lines()
        .filter_map(|line| read_build(line, &triple, &suffix))
        .collect()
}

/// One line of the index, as the build it names, or nothing when it
/// names something else or names it in a way pnpm will not use as a file
/// name: the index decides what pnpm downloads and where it puts it, so
/// every part of it is checked rather than trusted.
fn read_build(line: &str, triple: &str, suffix: &str) -> Option<Build> {
    let (sha256, file) = line.split_once("  ")?;
    if sha256.len() != 64 || !sha256.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    let named = file.strip_prefix("cpython-")?.strip_suffix(suffix)?;
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
        file: file.to_string(),
        sha256: sha256.to_string(),
    })
}

/// What python-build-standalone calls the interpreter of this machine.
/// `None` where it builds none, which is where pnpm installs none.
fn host_triple() -> Option<String> {
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
    let parent = directory.parent().expect("an installed interpreter has a parent directory");
    std::fs::create_dir_all(parent).into_diagnostic()?;
    let staged = tempfile::TempDir::new_in(parent).into_diagnostic()?;
    let file = std::fs::File::open(archive).into_diagnostic()?;
    let mut unpacked = tar::Archive::new(flate2::read::GzDecoder::new(file));
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

#[cfg(test)]
mod tests;
