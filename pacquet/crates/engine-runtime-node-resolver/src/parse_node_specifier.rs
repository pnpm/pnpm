//! Pacquet port of
//! [`parseNodeSpecifier.ts`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/parseNodeSpecifier.ts).

use derive_more::{Display, Error};
use miette::Diagnostic;

/// One of nodejs.org's published release channels.
///
/// Pacquet keeps the value as a `String` (rather than a closed enum)
/// because the upstream parser passes the channel through to the
/// mirror URL builder unchanged — the set is closed today but the
/// validation happens at parse time, not after.
pub const RELEASE_CHANNELS: &[&str] = &["nightly", "rc", "test", "v8-canary", "release"];

/// Parsed form of a `runtime:` bare specifier's body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSpecifier {
    pub release_channel: String,
    pub version_specifier: String,
}

/// Errors raised by [`parse_node_specifier`].
///
/// Matches upstream's `INVALID_NODE_RELEASE_CHANNEL` code so log
/// consumers (e.g. `@pnpm/cli.default-reporter`) parse the same string.
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
pub enum ParseNodeSpecifierError {
    #[display("\"{channel}\" is not a valid Node.js release channel")]
    #[diagnostic(
        code(INVALID_NODE_RELEASE_CHANNEL),
        help("Valid release channels are: nightly, rc, test, v8-canary, release")
    )]
    InvalidReleaseChannel {
        #[error(not(source))]
        channel: String,
    },
}

/// Split a `runtime:` bare specifier's body into its release-channel
/// and version-selector halves.
pub fn parse_node_specifier(specifier: &str) -> Result<NodeSpecifier, ParseNodeSpecifierError> {
    if let Some((channel, rest)) = specifier.split_once('/') {
        if !RELEASE_CHANNELS.contains(&channel) {
            return Err(ParseNodeSpecifierError::InvalidReleaseChannel {
                channel: channel.to_string(),
            });
        }
        return Ok(NodeSpecifier {
            release_channel: channel.to_string(),
            version_specifier: rest.to_string(),
        });
    }

    if let Some(channel) = prerelease_channel(specifier) {
        return Ok(NodeSpecifier {
            release_channel: channel.to_string(),
            version_specifier: specifier.to_string(),
        });
    }

    if is_stable_version(specifier) {
        return Ok(NodeSpecifier {
            release_channel: "release".to_string(),
            version_specifier: specifier.to_string(),
        });
    }

    if RELEASE_CHANNELS.contains(&specifier) {
        return Ok(NodeSpecifier {
            release_channel: specifier.to_string(),
            version_specifier: "latest".to_string(),
        });
    }

    Ok(NodeSpecifier {
        release_channel: "release".to_string(),
        version_specifier: specifier.to_string(),
    })
}

/// Return the prerelease channel for an exact `X.Y.Z-<channel>...`
/// version, or `None` if `specifier` is not an exact prerelease.
///
/// Mirrors upstream's regex `^\d+\.\d+\.\d+-(nightly|rc|test|v8-canary)`.
fn prerelease_channel(specifier: &str) -> Option<&'static str> {
    let (head, suffix) = specifier.split_once('-')?;
    if !is_stable_version(head) {
        return None;
    }
    for candidate in &["nightly", "rc", "test", "v8-canary"] {
        if suffix == *candidate || suffix.starts_with(&format!("{candidate}.")) {
            return Some(candidate);
        }
        // `v8-canary` has a dot-less continuation: e.g.
        // `22.0.0-v8-canary20250101abc`. Same for the `nightly` build
        // identifiers like `22.0.0-nightly20250315abcdef`. Allow any
        // trailing characters after the channel name.
        if let Some(rest) = suffix.strip_prefix(candidate)
            && rest.chars().next().is_none_or(|next| !next.is_ascii_alphabetic() || next == '.')
        {
            return Some(candidate);
        }
    }
    None
}

fn is_stable_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let major = parts.next();
    let minor = parts.next();
    let patch = parts.next();
    if parts.next().is_some() {
        return false;
    }
    matches!((major, minor, patch),
        (Some(m), Some(n), Some(p))
        if !m.is_empty() && m.bytes().all(|byte| byte.is_ascii_digit())
        && !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit())
        && !p.is_empty() && p.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests;
