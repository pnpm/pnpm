//! Downloading a release's archives and unpacking them into one toolchain
//! directory.

use crate::{Channel, RustToolchainError, ToolchainRequest, manifest::Archive};
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_tarball::BoundedReader;
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs, io,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// `rustc`, the largest archive, is about 130 MB.
const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;

/// What one archive may unpack to, in the two costs its compressed size
/// does not bound.
const MAX_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 200_000;

/// Where the toolchain `pinned` names is installed for `request`.
///
/// The name ends in a digest of the components and targets, since two
/// projects pinning the same release may ask for different ones. It is built
/// from validated parts only, so no manifest can name a directory of its own.
pub(crate) fn toolchain_dir(
    toolchains: &Path,
    pinned: &Channel,
    host: &str,
    request: &ToolchainRequest,
) -> PathBuf {
    toolchains.join(format!("{pinned}-{host}-{}", selection_digest(request)))
}

fn selection_digest(request: &ToolchainRequest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(request.profile.as_str());
    for name in request.components
        .iter()
        .chain([&String::new()])
        .chain(&request.targets)
    {
        hasher.update(b"\n");
        hasher.update(name);
    }
    hasher.finalize()[..4]
        .iter()
        .fold(String::new(), |mut digest, byte| {
            write!(digest, "{byte:02x}").expect("writing to a String does not fail");
            digest
        })
}

/// The newest installed toolchain the channel `request` names may resolve
/// to, for an install that cannot ask the distribution server which one it
/// resolves to now.
pub(crate) fn newest_installed(
    toolchains: &Path,
    host: &str,
    request: &ToolchainRequest,
) -> Option<PathBuf> {
    let suffix = format!("-{host}-{}", selection_digest(request));
    fs::read_dir(toolchains)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let pinned = Channel::parse(name.strip_suffix(&suffix)?)?;
            request.channel
                .accepts(&pinned)
                .then(|| (release_order(&pinned), entry.path()))
        })
        .max()
        .map(|(_, dir)| dir)
}

/// How long a channel that moves is taken to resolve to the release it
/// resolved to last, before the distribution server is asked again.
const CHANNEL_MAX_AGE: Duration = Duration::from_hours(24);

/// Where the release a moving channel last resolved to is recorded.
fn resolution_record(toolchains: &Path, host: &str, request: &ToolchainRequest) -> PathBuf {
    toolchains
        .join(".channels")
        .join(format!("{}-{host}-{}", request.channel, selection_digest(request)))
}

/// The toolchain a moving channel resolved to within the last
/// [`CHANNEL_MAX_AGE`], if it is still installed.
pub(crate) fn recent_resolution(
    toolchains: &Path,
    host: &str,
    request: &ToolchainRequest,
) -> Option<PathBuf> {
    let record = resolution_record(toolchains, host, request);
    let age = fs::metadata(&record)
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?;
    if age > CHANNEL_MAX_AGE {
        return None;
    }
    let pinned = Channel::parse(&fs::read_to_string(&record).ok()?)?;
    let dir = toolchain_dir(toolchains, &pinned, host, request);
    (request.channel.accepts(&pinned) && dir.is_dir()).then_some(dir)
}

/// Record what a moving channel resolved to. A record that cannot be
/// written only costs the next install a manifest download.
pub(crate) fn record_resolution(
    toolchains: &Path,
    host: &str,
    request: &ToolchainRequest,
    pinned: &Channel,
) {
    let record = resolution_record(toolchains, host, request);
    if let Some(parent) = record.parent()
        && fs::create_dir_all(parent).is_ok()
    {
        let _ = pnpm_fs::write_atomic(&record, pinned.to_string().as_bytes());
    }
}

fn release_order(pinned: &Channel) -> (u64, u64, u64, String) {
    match pinned {
        Channel::Version { major, minor, patch } => {
            (*major, *minor, patch.unwrap_or(0), String::new())
        }
        Channel::Named { date, .. } => (0, 0, 0, date.clone().unwrap_or_default()),
    }
}

/// Download every archive and unpack them together into `dir`.
pub(crate) async fn install<Reporter: self::Reporter>(
    config: &Config,
    client: &ThrottledClient,
    pinned: &Channel,
    host: &str,
    archives: &[Archive],
    dir: &Path,
) -> Result<(), RustToolchainError> {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Info,
        message: format!("Installing Rust {pinned} ({host})"),
    }));
    let install_error = |error: io::Error| RustToolchainError::Install {
        dir: dir.to_path_buf(),
        error: Arc::new(error),
    };
    let parent = dir.parent().expect("a toolchain directory has a parent directory");
    fs::create_dir_all(parent).map_err(install_error)?;
    let staged = tempfile::TempDir::new_in(parent).map_err(install_error)?;
    for archive in archives {
        let body = crate::manifest::get(config, client, &archive.url, MAX_ARCHIVE_BYTES).await?;
        let mut checker = ssri::IntegrityChecker::new(archive.integrity.clone());
        checker.input(&body);
        if checker.result().is_err() {
            return Err(RustToolchainError::IntegrityMismatch { url: archive.url.clone() });
        }
        let destination = staged.path().to_path_buf();
        let url = archive.url.clone();
        tokio::task::spawn_blocking(move || unpack(&body, &destination)).await
            .map_err(|error| install_error(io::Error::other(error)))?
            .map_err(|reason| RustToolchainError::Unpack { url, reason })?;
    }
    match fs::rename(staged.keep(), dir) {
        Ok(()) => Ok(()),
        // Another install of the same toolchain won the race, and unpacked
        // the same archives this one did.
        Err(_) if dir.is_dir() => Ok(()),
        Err(error) => Err(install_error(error)),
    }
}

/// Unpack the files of one component archive into `destination`.
///
/// An archive holds `<name>-<version>-<target>/<component>/...` beside the
/// installer's own files. Only what is below a component directory is
/// installed, as rustup installs it, without the component's file list.
pub(crate) fn unpack(gzipped: &[u8], destination: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(BoundedReader::new(
        flate2::read::GzDecoder::new(gzipped),
        MAX_UNPACKED_BYTES,
    ));
    archive.set_max_metadata_size(Some(pnpm_tarball::MAX_TARBALL_METADATA_BYTES));
    archive.set_preserve_permissions(true);
    let entries = archive.entries().map_err(|error| error.to_string())?;
    for (count, entry) in entries.enumerate() {
        if count >= MAX_ENTRIES {
            return Err(format!("it holds more than {MAX_ENTRIES} entries"));
        }
        let mut entry = entry.map_err(|error| error.to_string())?;
        let path = entry
            .path()
            .map_err(|error| error.to_string())?
            .into_owned();
        let Some(relative) = installed_path(&path)? else { continue };
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            continue;
        }
        if !entry_type.is_file() {
            return Err(format!("{} is not a regular file", path.display()));
        }
        let target = destination.join(relative);
        let parent = target.parent().expect("an installed file has a parent directory");
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        entry
            .unpack(&target)
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

/// Where an archive entry is installed, relative to the toolchain
/// directory. `None` for what is not installed. A path that would leave the
/// toolchain directory is refused.
fn installed_path(path: &Path) -> Result<Option<PathBuf>, String> {
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => segments.push(segment),
            Component::CurDir => {}
            _ => return Err(format!("{} leaves the archive", path.display())),
        }
    }
    let [_, _, rest @ ..] = segments.as_slice() else { return Ok(None) };
    if rest.is_empty() || rest == [std::ffi::OsStr::new("manifest.in")] {
        return Ok(None);
    }
    Ok(Some(rest.iter().collect()))
}

#[cfg(test)]
mod tests;
