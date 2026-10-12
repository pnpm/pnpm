//! The channel manifest a Rust release is described by, verified against
//! the Rust project's release signing key before anything in it is used.
//!
//! <https://forge.rust-lang.org/infra/channel-layout.html>

use crate::{Channel, ChannelName, RustToolchainError, ToolchainRequest, channel::parse_date};
use pnpm_config::{Config, DEFAULT_RUST_DIST_SERVER, Tool};
use pnpm_crypto_shasums_file::{TrustedReleaseKey, is_signed_by_trusted_key};
use pnpm_network::{AuthHeaders, ThrottledClient};
use serde::Deserialize;
use ssri::{Algorithm, Integrity};
use std::{collections::BTreeMap, sync::Arc};

/// The Rust project's tag and release signing key, as
/// <https://static.rust-lang.org/rust-key.gpg.ascii> publishes it.
const RUST_RELEASE_KEYS: &[TrustedReleaseKey] = &[TrustedReleaseKey {
    fingerprint: "108F66205EAEB0AAA8DD5E1C85AB96E6FA1BE5FE",
    armored_key: include_str!("rust-release-key.asc"),
}];

/// A manifest is about a megabyte. Anything far past that is not one.
const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: usize = 64 * 1024;

/// The distribution server releases are read from: `tools.rust.mirror`, or
/// the Rust project's own.
pub(crate) fn dist_server(config: &Config) -> &str {
    config.tool_mirror(Tool::Rust).unwrap_or(DEFAULT_RUST_DIST_SERVER)
}

/// One archive of a release, and the hash the signed manifest names for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Archive {
    pub(crate) url: String,
    pub(crate) integrity: Integrity,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Manifest {
    #[serde(skip)]
    url: String,
    date: String,
    pkg: BTreeMap<String, Package>,
    #[serde(default)]
    renames: BTreeMap<String, Rename>,
}

#[derive(Debug, Deserialize)]
struct Package {
    #[serde(default)]
    version: String,
    #[serde(default)]
    target: BTreeMap<String, TargetBuild>,
}

#[derive(Debug, Deserialize)]
struct TargetBuild {
    available: bool,
    #[serde(default)]
    url: String,
    #[serde(default)]
    hash: String,
}

#[derive(Debug, Deserialize)]
struct Rename {
    to: String,
}

/// Download the manifest of `channel` and its detached signature, and parse
/// the manifest once the signature proves it is a Rust release.
pub(crate) async fn fetch(
    config: &Config,
    client: &ThrottledClient,
    server: &str,
    channel: &Channel,
) -> Result<Manifest, RustToolchainError> {
    let url = format!("{server}/{}", channel.manifest_path());
    let body = get(config, client, &url, MAX_MANIFEST_BYTES).await?;
    let signature_url = format!("{url}.asc");
    let signature = get(config, client, &signature_url, MAX_SIGNATURE_BYTES).await?;
    parse_signed(url, &body, &signature)
}

pub(crate) fn parse_signed(
    url: String,
    body: &[u8],
    signature: &[u8],
) -> Result<Manifest, RustToolchainError> {
    let signed = is_signed_by_trusted_key(body, signature, RUST_RELEASE_KEYS)
        .map_err(|error| RustToolchainError::SignatureUnreadable { url: url.clone(), error })?;
    if !signed {
        return Err(RustToolchainError::SignatureInvalid { url });
    }
    let invalid = |reason: String| RustToolchainError::InvalidManifest { url: url.clone(), reason };
    let text = std::str::from_utf8(body).map_err(|error| invalid(error.to_string()))?;
    let mut manifest: Manifest =
        toml::from_str(text).map_err(|error| invalid(error.message().to_string()))?;
    manifest.url = url;
    Ok(manifest)
}

/// A complete GET of `url`, refused past `limit` bytes.
pub(crate) async fn get(
    config: &Config,
    client: &ThrottledClient,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, RustToolchainError> {
    let response = client
        .get_limited_bytes_with_secure_auth_and_retry(
            url,
            &AuthHeaders::default(),
            None,
            config.retry_opts(),
            limit,
        )
        .await
        .map_err(|error| RustToolchainError::Network {
            url: url.to_string(),
            error: Arc::new(error),
        })?;
    if response.body_truncated {
        return Err(RustToolchainError::TooLarge { url: url.to_string(), limit });
    }
    if !response.status.is_success() {
        return Err(RustToolchainError::StatusNotOk {
            url: url.to_string(),
            status: response.status.as_u16(),
        });
    }
    Ok(response.body)
}

impl Manifest {
    /// The pinned name of the release this manifest describes, which is
    /// what a channel that moves resolved to.
    ///
    /// The signature proves the manifest is a Rust release, not that it is
    /// the release `channel` names: a mirror could answer with an older one.
    /// So the release the manifest describes has to be one `channel` names.
    pub(crate) fn pinned(&self, channel: &Channel) -> Result<Channel, RustToolchainError> {
        let invalid =
            |reason: String| RustToolchainError::InvalidManifest { url: self.url.clone(), reason };
        let version = self.pkg
            .get("rustc")
            .and_then(|rustc| rustc.version.split_whitespace().next())
            .ok_or_else(|| invalid("the rustc package names no release version".to_string()))?;
        let date =
            parse_date(&self.date).ok_or_else(|| invalid("`date` is not a date".to_string()))?;
        release_named_by(channel, version, date)
            .ok_or_else(|| invalid(format!("it describes Rust {version} of {date}, not {channel}")))
    }

    /// The archives to unpack into a toolchain for `host` that `request`
    /// asks for.
    ///
    /// A component the profile brings in is left out where the release does
    /// not publish it, as rustup does. One the file lists is required.
    pub(crate) fn archives(
        &self,
        server: &str,
        pinned: &Channel,
        host: &str,
        request: &ToolchainRequest,
    ) -> Result<Vec<Archive>, RustToolchainError> {
        let mut archives = Vec::new();
        for component in request.profile.components() {
            let required = matches!(*component, "rustc" | "cargo" | "rust-std");
            match self.archive(server, component, host) {
                Some(archive) => archives.push(archive?),
                None if required => {
                    return Err(RustToolchainError::NotPublishedForHost {
                        channel: pinned.to_string(),
                        target: host.to_string(),
                    });
                }
                None => {}
            }
        }
        let requested = request.components
            .iter()
            .map(|component| (component.as_str(), host))
            .chain(
                request.targets
                    .iter()
                    .map(|target| ("rust-std", target.as_str())),
            );
        for (component, target) in requested {
            let archive = self
                .archive(server, component, target)
                .ok_or_else(|| RustToolchainError::ComponentUnavailable {
                    channel: pinned.to_string(),
                    component: component.to_string(),
                    target: target.to_string(),
                })?;
            archives.push(archive?);
        }
        let mut seen = std::collections::HashSet::new();
        archives.retain(|archive| seen.insert(archive.url.clone()));
        Ok(archives)
    }

    fn archive(
        &self,
        server: &str,
        component: &str,
        target: &str,
    ) -> Option<Result<Archive, RustToolchainError>> {
        let name =
            self.renames.get(component).map_or(component, |rename| rename.to.as_str());
        let targets = &self.pkg.get(name)?.target;
        let build = targets
            .get(target)
            .or_else(|| targets.get("*"))
            .filter(|build| build.available && !build.url.is_empty())?;
        Some(self.read_build(server, build))
    }

    fn read_build(&self, server: &str, build: &TargetBuild) -> Result<Archive, RustToolchainError> {
        let integrity = (build.hash.len() == 64)
            .then(|| Integrity::from_hex(&build.hash, Algorithm::Sha256).ok())
            .flatten()
            .ok_or_else(|| RustToolchainError::InvalidManifest {
                url: self.url.clone(),
                reason: format!("{} has no SHA-256 hash", build.url),
            })?;
        let url = match build.url.strip_prefix(DEFAULT_RUST_DIST_SERVER) {
            Some(path) if path.starts_with('/') => format!("{server}{path}"),
            _ => build.url.clone(),
        };
        Ok(Archive { url, integrity })
    }
}

/// The pinned name of the release of `version` published on `date`, if
/// `channel` names that release.
fn release_named_by(channel: &Channel, version: &str, date: &str) -> Option<Channel> {
    let Channel::Named { name, date: wanted } = channel else {
        return stable_release(version)
            .filter(|release| channel == release || channel.accepts(release));
    };
    if wanted
        .as_deref()
        .is_some_and(|wanted| wanted != date)
    {
        return None;
    }
    match name {
        ChannelName::Stable if wanted.is_some() => stable_release(version).map(|_| channel.clone()),
        ChannelName::Stable => stable_release(version),
        ChannelName::Beta => prerelease(*name, "-beta", version, date),
        ChannelName::Nightly => prerelease(*name, "-nightly", version, date),
    }
}

/// The dated release of `name` that `version` is, if its version carries
/// the channel's `marker`.
fn prerelease(name: ChannelName, marker: &str, version: &str, date: &str) -> Option<Channel> {
    version
        .contains(marker)
        .then(|| Channel::Named { name, date: Some(date.to_string()) })
}

/// `version` as the pinned name of a stable release, which a beta or
/// nightly version is not.
fn stable_release(version: &str) -> Option<Channel> {
    Channel::parse(version)
        .filter(|release| matches!(release, Channel::Version { patch: Some(_), .. }))
}

#[cfg(test)]
mod tests;
