//! The release channel a toolchain file names, in the forms rustup accepts
//! for a release published on the Rust distribution server.

use std::fmt;

/// A channel as a toolchain file writes it. Every part is validated, so the
/// name is safe to put into a URL and a directory name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// `1.90.0`, or `1.90` for the newest patch release of that line.
    Version { major: u64, minor: u64, patch: Option<u64> },
    /// `stable`, `beta`, or `nightly`, optionally pinned to the date of one
    /// of its releases (`nightly-2025-01-01`).
    Named { name: ChannelName, date: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelName {
    Stable,
    Beta,
    Nightly,
}

impl ChannelName {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Nightly => "nightly",
        }
    }
}

impl Channel {
    /// `None` for a name rustup resolves without the distribution server,
    /// such as a toolchain linked with `rustup toolchain link`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if let Some(channel) = parse_named(text) {
            return Some(channel);
        }
        let mut parts = text.split('.');
        let major = parse_number(parts.next()?)?;
        let minor = parse_number(parts.next()?)?;
        let patch = match parts.next() {
            Some(patch) => Some(parse_number(patch)?),
            None => None,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self::Version { major, minor, patch })
    }

    /// Whether every install of this channel gets the same release. A channel
    /// that is not pinned moves when a new release is published.
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        match self {
            Self::Version { patch, .. } => patch.is_some(),
            Self::Named { date, .. } => date.is_some(),
        }
    }

    /// The path of the channel's manifest below the distribution server.
    #[must_use]
    pub fn manifest_path(&self) -> String {
        match self {
            Self::Named { name, date: Some(date) } => {
                format!("dist/{date}/channel-rust-{}.toml", name.as_str())
            }
            _ => format!("dist/channel-rust-{self}.toml"),
        }
    }

    /// Whether `pinned`, the name of an installed release, is one this
    /// channel may resolve to.
    #[must_use]
    pub fn accepts(&self, pinned: &Self) -> bool {
        match (self, pinned) {
            (
                Self::Version { major, minor, patch: None },
                Self::Version {
                    major: pinned_major,
                    minor: pinned_minor,
                    patch: Some(_),
                },
            ) => major == pinned_major && minor == pinned_minor,
            (
                Self::Named {
                    name: ChannelName::Stable,
                    date: None,
                },
                Self::Version { patch: Some(_), .. },
            ) => true,
            (
                Self::Named { name, date: None },
                Self::Named { name: pinned_name, date: Some(_) },
            ) => name == pinned_name,
            _ => self == pinned,
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version { major, minor, patch: Some(patch) } => {
                write!(formatter, "{major}.{minor}.{patch}")
            }
            Self::Version { major, minor, patch: None } => write!(formatter, "{major}.{minor}"),
            Self::Named { name, date: Some(date) } => write!(formatter, "{}-{date}", name.as_str()),
            Self::Named { name, date: None } => formatter.write_str(name.as_str()),
        }
    }
}

fn parse_named(text: &str) -> Option<Channel> {
    let (name, date) = match text.split_once('-') {
        Some((name, date)) => (name, Some(date)),
        None => (text, None),
    };
    let name = match name {
        "stable" => ChannelName::Stable,
        "beta" => ChannelName::Beta,
        "nightly" => ChannelName::Nightly,
        _ => return None,
    };
    let date = match date {
        Some(date) => Some(parse_date(date)?.to_string()),
        None => None,
    };
    Some(Channel::Named { name, date })
}

/// `YYYY-MM-DD`, the form the distribution server names release dates in.
pub(crate) fn parse_date(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let well_formed = bytes.len() == 10
        && bytes
            .iter()
            .enumerate()
            .all(
                |(index, byte)| {
                    if index == 4 || index == 7 { *byte == b'-' } else { byte.is_ascii_digit() }
                },
            );
    well_formed.then_some(text)
}

fn parse_number(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    canonical
        .then(|| text.parse().ok())
        .flatten()
}

#[cfg(test)]
mod tests;
