//! Installs the Rust toolchain a project's rustup toolchain file names.
//!
//! A release is read from the channel manifest the Rust project publishes
//! and signs. Its components are downloaded, checked against the hashes the
//! manifest names, and unpacked together into one directory under
//! `<store>/rust`, the layout rustup gives an installed toolchain. A
//! toolchain directory is complete or absent: it is unpacked beside where it
//! belongs and moved there.

#[cfg(target_family = "wasm")]
extern crate pnpm_http as reqwest;

pub use channel::{Channel, ChannelName};
pub use toolchain_file::{
    Profile, ToolchainRequest, Unmanaged, find_toolchain_file, read_toolchain_file,
};

mod channel;
mod host;
mod install;
mod manifest;
mod toolchain_file;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_crypto_shasums_file::ReleaseSignatureError;
use pnpm_network::ThrottledClient;
use pnpm_reporter::Reporter;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The directory, beside a toolchain file, that pnpm links the toolchain the
/// file names into.
pub const TOOLCHAIN_LINK: [&str; 2] = [".pnpm", "rust"];

/// A toolchain pnpm installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledToolchain {
    pub dir: PathBuf,
}

impl InstalledToolchain {
    #[must_use]
    pub fn bin_dir(&self) -> PathBuf {
        self.dir.join("bin")
    }

    /// The path of one of the toolchain's executables, such as `cargo`.
    #[must_use]
    pub fn executable(&self, name: &str) -> PathBuf {
        self.bin_dir()
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    }
}

/// Install the toolchain `request` asks for, or find the one an earlier
/// install put in the store.
///
/// A channel that moves, such as `stable`, is read from the distribution
/// server again once a day, so that a new release is picked up. Offline, the
/// newest installed release of the channel is used.
pub async fn install_toolchain<Reporter: self::Reporter>(
    config: &Config,
    client: &ThrottledClient,
    request: &ToolchainRequest,
) -> Result<InstalledToolchain, RustToolchainError> {
    let host = host::host_triple().ok_or(RustToolchainError::UnsupportedHost)?;
    install_for_host::<Reporter>(config, client, request, &host).await
}

async fn install_for_host<Reporter: self::Reporter>(
    config: &Config,
    client: &ThrottledClient,
    request: &ToolchainRequest,
    host: &str,
) -> Result<InstalledToolchain, RustToolchainError> {
    let toolchains = toolchains_dir(config);
    if let Some(dir) = installed_without_download(config, &toolchains, host, request) {
        return Ok(InstalledToolchain { dir });
    }
    if config.offline {
        return Err(RustToolchainError::Offline { channel: request.channel.to_string() });
    }
    let server = manifest::dist_server(config);
    let manifest = manifest::fetch(config, client, server, &request.channel).await?;
    let pinned = manifest.pinned(&request.channel)?;
    refuse_older_release(&toolchains, host, request, &pinned)?;
    let dir = install::toolchain_dir(&toolchains, &pinned, host, request);
    if !dir.is_dir() {
        let archives = manifest.archives(server, &pinned, host, request)?;
        install::install::<Reporter>(config, client, &pinned, host, &archives, &dir).await?;
    }
    if !request.channel.is_pinned() {
        install::record_resolution(&toolchains, host, request, &pinned);
    }
    Ok(InstalledToolchain { dir })
}

/// The installed toolchain that answers `request` without asking the
/// distribution server: a pinned release, a moving channel resolved within
/// the last day, or offline the newest installed release of the channel.
fn installed_without_download(
    config: &Config,
    toolchains: &Path,
    host: &str,
    request: &ToolchainRequest,
) -> Option<PathBuf> {
    if request.channel.is_pinned() {
        let dir = install::toolchain_dir(toolchains, &request.channel, host, request);
        return dir.is_dir().then_some(dir);
    }
    install::recent_resolution(toolchains, host, request)
        .or_else(|| {
            config.offline.then(|| install::newest_installed(toolchains, host, request)).flatten()
        })
}

/// A channel never moves back to an older release, so one that appears to
/// was answered by a mirror serving an older manifest.
fn refuse_older_release(
    toolchains: &Path,
    host: &str,
    request: &ToolchainRequest,
    pinned: &Channel,
) -> Result<(), RustToolchainError> {
    match install::last_resolution(toolchains, host, request) {
        Some(last) if install::is_older(pinned, &last) => Err(RustToolchainError::OlderRelease {
            channel: request.channel.to_string(),
            release: pinned.to_string(),
            last: last.to_string(),
        }),
        _ => Ok(()),
    }
}

/// The `bin` directory of the toolchain pnpm linked for `dir`: the one beside
/// the toolchain file rustup reads there, searched for no higher than
/// `boundary`.
///
/// The link is taken only when it leads to a toolchain in `config`'s store
/// that the toolchain file asks for now. Neither a toolchain the checkout
/// committed nor a link left behind by an earlier install is run as the one
/// the file names.
#[must_use]
pub fn linked_bin_dir(config: &Config, dir: &Path, boundary: &Path) -> Option<PathBuf> {
    let file = find_toolchain_file(dir, boundary)?;
    let request = read_toolchain_file(&file).ok()?.ok()?;
    let link = file
        .parent()?
        .join(TOOLCHAIN_LINK.iter().collect::<PathBuf>());
    let target = dunce::canonicalize(&link).ok()?;
    let toolchains = dunce::canonicalize(toolchains_dir(config)).ok()?;
    let host = host::host_triple()?;
    let installed = target.parent() == Some(toolchains.as_path())
        && target
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| install::is_installation_of(name, &host, &request));
    let bin_dir = link.join("bin");
    (installed && bin_dir.is_dir()).then_some(bin_dir)
}

/// Where the release `pinned` is installed for `request` on this machine.
/// `None` where Rust publishes no toolchain for it.
#[must_use]
pub fn installation_dir(
    config: &Config,
    pinned: &Channel,
    request: &ToolchainRequest,
) -> Option<PathBuf> {
    let host = host::host_triple()?;
    Some(install::toolchain_dir(&toolchains_dir(config), pinned, &host, request))
}

fn toolchains_dir(config: &Config) -> PathBuf {
    config.store_dir.root().join("rust")
}

/// Errors raised while reading a toolchain file or installing the toolchain
/// it names.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RustToolchainError {
    #[display("Failed to read {}", file.display())]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_FILE_INVALID))]
    ReadToolchainFile {
        file: PathBuf,
        #[error(source)]
        error: Arc<io::Error>,
    },

    #[display("{} is not a valid toolchain file: {reason}", file.display())]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_FILE_INVALID))]
    InvalidToolchainFile {
        file: PathBuf,
        #[error(not(source))]
        reason: String,
    },

    #[display("Rust toolchains are not published for this machine")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_UNAVAILABLE))]
    UnsupportedHost,

    #[display("Rust {channel} is not in the store, and the install is offline")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_OFFLINE))]
    Offline {
        #[error(not(source))]
        channel: String,
    },

    #[display("Failed to download {url}")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_FETCH_FAIL))]
    Network {
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("Failed to download {url} (status: {status})")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_FETCH_FAIL))]
    StatusNotOk {
        #[error(not(source))]
        url: String,
        status: u16,
    },

    #[display("{url} exceeds {limit} bytes")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_FETCH_FAIL))]
    TooLarge {
        #[error(not(source))]
        url: String,
        limit: usize,
    },

    #[display(
        "The OpenPGP signature of {url} does not match the Rust release key. The toolchain cannot be verified as a genuine Rust release."
    )]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_SIGNATURE_INVALID))]
    SignatureInvalid {
        #[error(not(source))]
        url: String,
    },

    #[display("Could not check the OpenPGP signature of {url}")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_SIGNATURE_INVALID))]
    SignatureUnreadable {
        url: String,
        #[error(source)]
        error: ReleaseSignatureError,
    },

    #[display("The Rust release manifest {url} is invalid: {reason}")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_MANIFEST_INVALID))]
    InvalidManifest {
        #[error(not(source))]
        url: String,
        reason: String,
    },

    #[display(
        "Rust {channel} resolved to {release}, older than {last} it resolved to before. A channel does not move back to an older release."
    )]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_MANIFEST_INVALID))]
    OlderRelease {
        #[error(not(source))]
        channel: String,
        release: String,
        last: String,
    },

    #[display("Rust {channel} is not published for {target}")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_UNAVAILABLE))]
    NotPublishedForHost {
        #[error(not(source))]
        channel: String,
        target: String,
    },

    #[display("The {component} component of Rust {channel} is not published for {target}")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_UNAVAILABLE))]
    ComponentUnavailable {
        #[error(not(source))]
        channel: String,
        component: String,
        target: String,
    },

    #[display("{url} does not match the hash the signed Rust release manifest names")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_INTEGRITY))]
    IntegrityMismatch {
        #[error(not(source))]
        url: String,
    },

    #[display("Failed to unpack {url}: {reason}")]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_UNPACK))]
    Unpack {
        #[error(not(source))]
        url: String,
        reason: String,
    },

    #[display("Failed to install the Rust toolchain into {}", dir.display())]
    #[diagnostic(code(ERR_PNPM_RUST_TOOLCHAIN_UNPACK))]
    Install {
        dir: PathBuf,
        #[error(source)]
        error: Arc<io::Error>,
    },
}

#[cfg(test)]
mod tests;
