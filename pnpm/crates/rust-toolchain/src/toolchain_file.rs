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

impl ToolchainRequest {
    /// The request for `channel` with rustup's default profile and nothing
    /// added to it.
    #[must_use]
    pub fn for_channel(channel: Channel) -> Self {
        Self { channel, profile: Profile::Default, components: Vec::new(), targets: Vec::new() }
    }

    /// The request with the standard libraries of `targets` added. A value
    /// that names no published target is left out: a target specification
    /// file, or `host-tuple`, which Cargo reads as the host's.
    #[must_use]
    pub fn with_targets<'a>(mut self, targets: impl IntoIterator<Item = &'a str>) -> Self {
        self.targets.extend(
            targets
                .into_iter()
                .filter(|target| is_published_target_name(target))
                .map(str::to_string),
        );
        self.targets.sort();
        self.targets.dedup();
        self
    }
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

/// The toolchain file `contents` (a new one for `None`) with its channel
/// set to `channel`. Everything else the file says, comments included, is
/// kept. A file that names a toolchain by `path`, or spells the table in a
/// form this edit does not read, is refused rather than rewritten.
pub fn with_channel(contents: Option<&str>, channel: &Channel) -> Result<String, String> {
    let Some(contents) = contents else {
        return Ok(format!("[toolchain]\n{}\n", channel_line(channel)));
    };
    let trimmed = contents.trim();
    // A one-line comment is TOML, not a channel name.
    if !trimmed.is_empty() && !trimmed.contains(['\n', '=', '[']) && !trimmed.starts_with('#') {
        return Ok(format!("{channel}\n"));
    }
    let updated = set_toolchain_channel(contents, channel)?;
    match parse_toolchain_file(&updated) {
        Ok(Ok(request)) if request.channel == *channel => Ok(updated),
        Ok(Err(Unmanaged::CustomPath)) => Err("it names a toolchain by `path`".to_string()),
        _ => Err("pnpm cannot update the `[toolchain]` table as it is written".to_string()),
    }
}

/// `contents` with the `channel` key of its `[toolchain]` table set,
/// replacing the line that holds it or adding one under the header.
fn set_toolchain_channel(contents: &str, channel: &Channel) -> Result<String, String> {
    let line = channel_line(channel);
    let mut lines: Vec<String> = contents
        .lines()
        .map(str::to_string)
        .collect();
    match locate_channel(&lines) {
        ChannelLocation::Line(index) => lines[index] = replaced_channel_line(&lines[index], &line),
        ChannelLocation::Header(index) => lines.insert(index + 1, line),
        ChannelLocation::NoTable => return Ok(with_toolchain_table(contents, &line)),
    }
    Ok(join_lines(&lines, contents))
}

fn channel_line(channel: &Channel) -> String {
    format!(r#"channel = "{channel}""#)
}

/// Where the `channel` key of the `[toolchain]` table is, or where it goes.
enum ChannelLocation {
    Line(usize),
    Header(usize),
    NoTable,
}

fn locate_channel(lines: &[String]) -> ChannelLocation {
    let mut header = None;
    let mut in_toolchain = false;
    for (index, text) in lines.iter().enumerate() {
        let trimmed = text.trim_start();
        if trimmed.starts_with('[') {
            in_toolchain = trimmed.split('#').next().map(str::trim) == Some("[toolchain]");
            header = header.or_else(|| in_toolchain.then_some(index));
        } else if in_toolchain && is_channel_key(trimmed) {
            return ChannelLocation::Line(index);
        }
    }
    header.map_or(ChannelLocation::NoTable, ChannelLocation::Header)
}

fn is_channel_key(line: &str) -> bool {
    line.split_once('=')
        .is_some_and(|(key, _)| key.trim() == "channel")
}

/// `line` in place of the channel line `text`, with its indentation and its
/// comment. A channel name never holds a `#`, so one on the line starts the
/// comment.
fn replaced_channel_line(text: &str, line: &str) -> String {
    let trimmed = text.trim_start();
    let indent = &text[..text.len() - trimmed.len()];
    match trimmed.find('#') {
        Some(start) => format!("{indent}{line} {}", &trimmed[start..]),
        None => format!("{indent}{line}"),
    }
}

/// `contents` with a `[toolchain]` table holding `line` appended.
fn with_toolchain_table(contents: &str, line: &str) -> String {
    let separator = if contents.is_empty() || contents.ends_with("\n\n") {
        ""
    } else if contents.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{contents}{separator}[toolchain]\n{line}\n")
}

fn join_lines(lines: &[String], original: &str) -> String {
    let mut joined = lines.join("\n");
    if original.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

fn is_published_target_name(name: &str) -> bool {
    is_component_name(name) && !name.ends_with(".json") && name != "host-tuple"
}

fn is_component_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests;
