//! rustup's toolchain file: `rust-toolchain.toml`, or the older
//! `rust-toolchain`, which may also hold nothing but a channel name.
//!
//! <https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file>

use crate::{Channel, RustToolchainError};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// rustup reads the first of these that a directory has.
const TOOLCHAIN_FILE_NAMES: [&str; 2] = ["rust-toolchain", "rust-toolchain.toml"];

/// The toolchain file rustup would read in `dir`: the nearest one at or
/// above it. Only `boundary`, the checkout being installed, and what is
/// inside it are searched, so a file outside the repository never decides
/// what is installed into it or run for it.
#[must_use]
pub fn find_toolchain_file(dir: &Path, boundary: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .take_while(|ancestor| ancestor.starts_with(boundary))
        .flat_map(|ancestor| TOOLCHAIN_FILE_NAMES.map(|name| ancestor.join(name)))
        .find(|file| file.is_file())
        .filter(|file| inside(file, boundary))
}

/// Whether `file`, with every link resolved, is still inside `boundary`: a
/// toolchain file the checkout links to a file elsewhere is not read.
fn inside(file: &Path, boundary: &Path) -> bool {
    dunce::canonicalize(file)
        .ok()
        .zip(dunce::canonicalize(boundary).ok())
        .is_some_and(|(file, boundary)| file.starts_with(boundary))
}

/// What a toolchain file asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainRequest {
    pub channel: Channel,
    pub profile: Profile,
    /// The components the file lists, deduplicated and sorted.
    pub components: Vec<String>,
    /// The targets whose standard library to install beside the host's,
    /// deduplicated and sorted.
    pub targets: Vec<String>,
}

/// The components a toolchain is installed with before the ones its file
/// lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Minimal,
    /// rustup's `default` profile without `rust-docs`: it is tens of
    /// thousands of files few builds read, and a file that wants it can list
    /// it.
    Default,
}

impl Profile {
    #[must_use]
    pub fn components(self) -> &'static [&'static str] {
        match self {
            Self::Minimal => &["rustc", "cargo", "rust-std"],
            Self::Default => &["rustc", "cargo", "rust-std", "rustfmt", "clippy"],
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Default => "default",
        }
    }
}

/// A toolchain file that pnpm installs nothing for, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unmanaged {
    /// `path` names a toolchain already on disk.
    CustomPath,
    /// No `channel`: rustup uses the machine's default toolchain.
    NoChannel,
    /// A name rustup resolves without the distribution server, such as a
    /// toolchain linked with `rustup toolchain link`.
    UnknownChannel(String),
    /// A profile pnpm does not install, such as `complete`.
    UnsupportedProfile(String),
}

#[derive(Deserialize)]
struct ToolchainFile {
    toolchain: ToolchainSection,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ToolchainSection {
    channel: Option<String>,
    path: Option<String>,
    #[serde(default)]
    components: Vec<String>,
    #[serde(default)]
    targets: Vec<String>,
    profile: Option<String>,
}

/// Read the toolchain file at `file`.
pub fn read_toolchain_file(
    file: &Path,
) -> Result<Result<ToolchainRequest, Unmanaged>, RustToolchainError> {
    let contents = fs::read_to_string(file)
        .map_err(|error| RustToolchainError::ReadToolchainFile {
            file: file.to_path_buf(),
            error: Arc::new(error),
        })?;
    parse_toolchain_file(&contents)
        .map_err(|reason| RustToolchainError::InvalidToolchainFile {
            file: file.to_path_buf(),
            reason,
        })
}

pub(crate) fn parse_toolchain_file(
    contents: &str,
) -> Result<Result<ToolchainRequest, Unmanaged>, String> {
    let trimmed = contents.trim();
    let section = if trimmed.contains(['\n', '=', '[']) {
        toml::from_str::<ToolchainFile>(contents).map_err(|error| error.message().to_string())?
            .toolchain
    } else {
        ToolchainSection { channel: Some(trimmed.to_string()), ..ToolchainSection::default() }
    };
    let ToolchainSection {
        channel,
        path,
        components,
        targets,
        profile,
    } = section;
    if channel.is_some() && path.is_some() {
        return Err("`channel` and `path` cannot both be set".to_string());
    }
    if path.is_some() {
        return Ok(Err(Unmanaged::CustomPath));
    }
    let Some(channel_name) = channel else {
        return Ok(Err(Unmanaged::NoChannel));
    };
    let Some(channel) = Channel::parse(&channel_name) else {
        return Ok(Err(Unmanaged::UnknownChannel(channel_name)));
    };
    let profile = match profile.as_deref() {
        None | Some("default") => Profile::Default,
        Some("minimal") => Profile::Minimal,
        Some(other) => return Ok(Err(Unmanaged::UnsupportedProfile(other.to_string()))),
    };
    let components = sorted_names(components)?;
    let targets = sorted_names(targets)?;
    Ok(Ok(ToolchainRequest { channel, profile, components, targets }))
}

/// Component or target names, deduplicated and sorted.
fn sorted_names(mut names: Vec<String>) -> Result<Vec<String>, String> {
    if let Some(name) = names
        .iter()
        .find(|name| !is_component_name(name))
    {
        return Err(format!("{name:?} is not a component or target name"));
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn is_component_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests;
