//! Resolves `node@runtime:<spec>` dependencies, including the
//! `latest`-channel shortcut.
//!
//! [`NodeResolver`] implements [`Resolver`] and ties the per-helper
//! pieces (parser, mirror picker, asset reader) together so the
//! default-resolver dispatcher can route `node@runtime:<spec>`
//! dependencies through it.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Version;
use pnpm_crypto_shasums_file::{
    FetchShasumsFileError, FetchVerifiedNodeShasumsError, fetch_shasums_file_cached,
    fetch_verified_node_shasums_file_cached,
};
use pnpm_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PlatformAssetResolution,
    PlatformAssetTarget, VariationsResolution,
};
use pnpm_network::ThrottledClient;
use pnpm_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};
use ssri::Integrity;

use crate::{
    get_node_artifact_address::{GetNodeArtifactAddressOptions, get_node_artifact_address},
    get_node_mirror::{
        DEFAULT_NODE_MIRROR_BASE_URL, UNOFFICIAL_NODE_MIRROR_BASE_URL, get_node_mirror,
    },
    parse_node_specifier::{NodeSpecifier, ParseNodeSpecifierError, parse_node_specifier},
    resolve_node_version::{ResolveNodeVersionError, resolve_node_version},
};

const RESOLVED_VIA: &str = "nodejs.org";
const BARE_SPEC_PREFIX: &str = "runtime:";

/// Errors emitted by [`NodeResolver::resolve`] / [`NodeResolver::resolve_latest`].
///
/// Each variant maps to one of the node-resolver error codes
/// (`ERR_PNPM_NO_OFFLINE_NODEJS_RESOLUTION`, `ERR_PNPM_NODEJS_VERSION_NOT_FOUND`,
/// `ERR_PNPM_INVALID_NODE_RELEASE_CHANNEL`, plus the network failure modes
/// surfaced by the shasums-file and release-index fetchers).
#[derive(Debug, Display, Error, Diagnostic)]
pub enum NodeResolverError {
    #[display("Offline Node.js resolution is not supported")]
    #[diagnostic(code(ERR_PNPM_NO_OFFLINE_NODEJS_RESOLUTION))]
    Offline,

    #[display("Could not find a Node.js version that satisfies {spec}")]
    #[diagnostic(code(ERR_PNPM_NODEJS_VERSION_NOT_FOUND))]
    VersionNotFound {
        #[error(not(source))]
        spec: String,
    },

    #[diagnostic(transparent)]
    InvalidReleaseChannel(#[error(source)] ParseNodeSpecifierError),

    #[diagnostic(transparent)]
    FetchReleaseIndex(#[error(source)] ResolveNodeVersionError),

    #[diagnostic(transparent)]
    FetchShasumsFile(#[error(source)] FetchShasumsFileError),

    #[diagnostic(transparent)]
    FetchVerifiedNodeShasums(#[error(source)] FetchVerifiedNodeShasumsError),

    #[display("Failed to parse integrity {integrity} for {file_name}")]
    #[diagnostic(code(ERR_PNPM_NODE_INTEGRITY_PARSE_FAILED))]
    ParseIntegrity {
        integrity: String,
        file_name: String,
        #[error(source)]
        error: Arc<ssri::Error>,
    },
}

/// Node.js runtime resolver entry point.
///
/// One instance per install. Owns the throttled HTTP client (so the
/// asset-list fetch contends with the rest of the install for the
/// same socket budget) and the `nodeDownloadMirrors` map a user may
/// have configured. `offline` is honored on every resolve — the
/// resolver fails fast rather than spinning a request that's going
/// to time out behind a proxy.
pub struct NodeResolver {
    pub http_client: Arc<ThrottledClient>,
    pub node_download_mirrors: HashMap<String, String>,
    pub offline: bool,
    /// The pnpm cache directory backing the per-version SHASUMS disk
    /// cache. `None` disables the cache and every resolve fetches the
    /// asset lists from the mirror.
    pub cache_dir: Option<PathBuf>,
}

impl NodeResolver {
    #[must_use]
    pub fn new(http_client: Arc<ThrottledClient>) -> Self {
        Self { http_client, node_download_mirrors: HashMap::new(), offline: false, cache_dir: None }
    }
}

impl Resolver for NodeResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(self.resolve_impl(wanted_dependency, opts))
    }

    fn resolve_latest<'a>(
        &'a self,
        query: &'a LatestQuery,
        opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(self.resolve_latest_impl(query, opts))
    }
}

impl NodeResolver {
    async fn resolve_impl(
        &self,
        wanted_dependency: &WantedDependency,
        _opts: &ResolveOptions,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(version_spec) = bare_runtime_spec(wanted_dependency, "node") else {
            return Ok(None);
        };

        // A fast path could reuse the lockfile-pinned
        // VariationsResolution unchanged when the current package is
        // already pinned and no update is requested. Pacquet doesn't
        // thread that signal through `ResolveOptions` yet, so every
        // resolve re-fetches the asset list. Add the fast path once the
        // seam carries it.

        let picked = self
            .pick_node_version(version_spec)
            .await
            .map_err(|err| Box::new(err) as ResolveError)?;
        let variants = match self
            .read_node_assets(&picked.mirror, &picked.version, &picked.release_channel)
            .await
        {
            Ok(variants) => variants,
            // An exact-specifier pick skipped the release index, so a
            // failed asset read is ambiguous: the version may simply not
            // exist. Consult the index now, purely to raise the same
            // `ERR_PNPM_NODEJS_VERSION_NOT_FOUND` the index-first path
            // raises for a nonexistent version; any other outcome
            // re-raises the asset error unchanged.
            Err(error) if picked.resolved_without_index => {
                let error = match resolve_node_version(
                    &self.http_client,
                    &picked.version,
                    Some(&picked.mirror),
                )
                .await
                {
                    Ok(None) => {
                        NodeResolverError::VersionNotFound { spec: version_spec.to_string() }
                    }
                    _ => error,
                };
                return Err(Box::new(error));
            }
            Err(error) => return Err(Box::new(error)),
        };
        let PickedNodeVersion { version, .. } = picked;
        let range = normalize_node_runtime_version_specifier(
            version_spec,
            &version,
            wanted_dependency.prev_specifier.as_deref(),
        );
        let resolution = LockfileResolution::Variations(VariationsResolution { variants });
        let manifest = serde_json::json!({
            "name": "node",
            "version": version,
            "bin": node_bins_for_current_os(current_platform()),
        });
        Ok(Some(ResolveResult {
            id: format!("node@runtime:{version}").into(),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(std::sync::Arc::new(manifest)),
            resolution,
            resolved_via: RESOLVED_VIA.to_string(),
            normalized_bare_specifier: Some(format!("runtime:{range}")),
            alias: wanted_dependency.alias.clone(),
            policy_violation: None,
        }))
    }

    /// Parse a `runtime:` version spec, pick the mirror for its release
    /// channel, and resolve the spec to a concrete version.
    ///
    /// An exact stable-release specifier is its own resolution, so the
    /// release-index fetch is skipped for it and existence is proven by
    /// the asset-list fetch that follows —
    /// [`resolve_impl`](Self::resolve_impl) maps that fetch's failure
    /// back to the canonical not-found error.
    async fn pick_node_version(
        &self,
        version_spec: &str,
    ) -> Result<PickedNodeVersion, NodeResolverError> {
        if self.offline {
            return Err(NodeResolverError::Offline);
        }
        let parsed =
            parse_node_specifier(version_spec).map_err(NodeResolverError::InvalidReleaseChannel)?;
        let mirror = get_node_mirror(Some(&self.node_download_mirrors), &parsed.release_channel);
        if let Some(version) = exact_release_version(&parsed) {
            return Ok(PickedNodeVersion {
                version,
                mirror,
                release_channel: parsed.release_channel,
                resolved_without_index: true,
            });
        }
        let version =
            resolve_node_version(&self.http_client, &parsed.version_specifier, Some(&mirror))
                .await
                .map_err(NodeResolverError::FetchReleaseIndex)?
                .ok_or_else(|| NodeResolverError::VersionNotFound {
                    spec: version_spec.to_string(),
                })?;
        Ok(PickedNodeVersion {
            version,
            mirror,
            release_channel: parsed.release_channel,
            resolved_without_index: false,
        })
    }

    /// The specifier an `add node@runtime:<spec>` request saves to the
    /// manifest: `runtime:` plus the picked version, pinned exactly unless
    /// the previous specifier (or the requested spec) carries a `^`/`~`
    /// operator. This is the same normalization [`Resolver::resolve`]
    /// reports through `normalized_bare_specifier`, computed without
    /// fetching the platform asset list.
    pub async fn resolve_save_specifier(
        &self,
        version_spec: &str,
        prev_specifier: Option<&str>,
    ) -> Result<String, NodeResolverError> {
        // An exact stable-release specifier normalizes to itself
        // (`normalize_node_runtime_version_specifier` returns the
        // resolved version verbatim when it equals the spec), so no
        // version pick is needed. Whether the version exists is settled
        // by the resolve that follows the save.
        let parsed =
            parse_node_specifier(version_spec).map_err(NodeResolverError::InvalidReleaseChannel)?;
        if let Some(version) = exact_release_version(&parsed) {
            return Ok(format!("{BARE_SPEC_PREFIX}{version}"));
        }
        let picked = self.pick_node_version(version_spec).await?;
        let range =
            normalize_node_runtime_version_specifier(version_spec, &picked.version, prev_specifier);
        Ok(format!("{BARE_SPEC_PREFIX}{range}"))
    }

    async fn resolve_latest_impl(
        &self,
        query: &LatestQuery,
        _opts: &ResolveOptions,
    ) -> Result<Option<LatestInfo>, ResolveError> {
        let Some(manifest_spec) = bare_runtime_spec(&query.wanted_dependency, "node") else {
            return Ok(None);
        };
        let spec_owned;
        let version_spec = if query.compatible {
            manifest_spec
        } else {
            spec_owned = "latest";
            spec_owned
        };
        let parsed = parse_node_specifier(version_spec).map_err(|err| {
            Box::new(NodeResolverError::InvalidReleaseChannel(err)) as ResolveError
        })?;
        let mirror = get_node_mirror(Some(&self.node_download_mirrors), &parsed.release_channel);
        let version =
            resolve_node_version(&self.http_client, &parsed.version_specifier, Some(&mirror))
                .await
                .map_err(|err| {
                    Box::new(NodeResolverError::FetchReleaseIndex(err)) as ResolveError
                })?;
        let Some(version) = version else {
            return Ok(Some(LatestInfo::default()));
        };
        Ok(Some(LatestInfo {
            latest_manifest: Some(std::sync::Arc::new(serde_json::json!({
                "name": "node",
                "version": version,
            }))),
        }))
    }

    /// Fetch the `SHASUMS256.txt` for the picked version on the active
    /// mirror, then optionally augment with musl variants from
    /// unofficial-builds when the active mirror is the official one.
    ///
    /// The musl branch only fires when the active mirror is the
    /// default one (custom mirrors are assumed to publish their own
    /// musl-or-not policy), and musl-fetch failures are swallowed
    /// because old releases simply don't have musl builds.
    async fn read_node_assets(
        &self,
        mirror: &str,
        version: &str,
        release_channel: &str,
    ) -> Result<Vec<PlatformAssetResolution>, NodeResolverError> {
        let mut assets = read_node_assets_from_mirror(
            &self.http_client,
            mirror,
            version,
            /* musl_only */ false,
            /* verify_signature */ release_channel == "release",
            self.cache_dir.as_deref(),
        )
        .await?;
        if mirror == DEFAULT_NODE_MIRROR_BASE_URL
            && let Ok(mut musl_assets) = read_node_assets_from_mirror(
                &self.http_client,
                UNOFFICIAL_NODE_MIRROR_BASE_URL,
                version,
                /* musl_only */ true,
                /* verify_signature */ false,
                self.cache_dir.as_deref(),
            )
            .await
        {
            assets.append(&mut musl_assets);
        }
        Ok(assets)
    }
}

/// The concrete version an exact stable-release specifier names, when
/// the specifier is already in canonical `X.Y.Z` form. Such a specifier
/// needs no release-index lookup: the index would resolve it to itself.
/// Prereleases are excluded — on the `release` channel they never
/// exist, so routing them through the index keeps the canonical
/// not-found error path.
fn exact_release_version(parsed: &NodeSpecifier) -> Option<String> {
    if parsed.release_channel != "release" {
        return None;
    }
    Version::parse(&parsed.version_specifier)
        .ok()
        .filter(|version| version.pre_release.is_empty() && version.build.is_empty())
        .map(|version| version.to_string())
        .filter(|version| *version == parsed.version_specifier)
}

/// A concrete Node.js version picked for a `runtime:` version spec, plus
/// the mirror and release channel it was picked from.
struct PickedNodeVersion {
    version: String,
    mirror: String,
    release_channel: String,
    /// `true` when the pick came from
    /// [`exact_release_version`] — the release index was never
    /// consulted, so the version's existence is still unproven.
    resolved_without_index: bool,
}

/// Strip `runtime:` from a `(alias, bareSpecifier)` pair when both
/// halves match the runtime contract. Returns `None` (defer to the
/// next resolver) for any other shape.
fn bare_runtime_spec<'a>(wanted: &'a WantedDependency, expected_alias: &str) -> Option<&'a str> {
    if wanted.alias.as_deref() != Some(expected_alias) {
        return None;
    }
    wanted.bare_specifier.as_deref().and_then(|spec| spec.strip_prefix(BARE_SPEC_PREFIX))
}

fn normalize_node_runtime_version_specifier(
    version_spec: &str,
    resolved_version: &str,
    prev_specifier: Option<&str>,
) -> String {
    if resolved_version == version_spec
        || matches!(Version::parse(resolved_version), Ok(version) if !version.pre_release.is_empty())
    {
        return resolved_version.to_string();
    }
    let source = prev_specifier
        .and_then(|specifier| specifier.strip_prefix(BARE_SPEC_PREFIX))
        .unwrap_or(version_spec);
    let spec = source.split_once('/').map_or(source, |(_, spec)| spec);
    let prefix = if spec.starts_with('^') {
        "^"
    } else if spec.starts_with('~') {
        "~"
    } else {
        ""
    };
    format!("{prefix}{resolved_version}")
}

/// Read the asset list for one mirror version and decode each row
/// into a [`PlatformAssetResolution`].
///
/// Rows are matched against the nodejs.org artifact pattern
/// `node-v<version>-<platform>-<arch>(-musl)?.(tar.gz|zip)`.
/// Files that don't match (e.g. `.pkg`, `.msi`, source tarballs) are
/// dropped. When `musl_only` is true, glibc builds are filtered out
/// so the asset list only carries the musl-specific variants the
/// caller asked for.
async fn read_node_assets_from_mirror(
    http_client: &ThrottledClient,
    node_mirror_base_url: &str,
    version: &str,
    musl_only: bool,
    verify_signature: bool,
    cache_dir: Option<&Path>,
) -> Result<Vec<PlatformAssetResolution>, NodeResolverError> {
    // The URL is pinned to one released version, which is what makes it
    // eligible for the SHASUMS disk cache.
    let integrities_url = format!("{node_mirror_base_url}v{version}/SHASUMS256.txt");
    let items = if verify_signature {
        fetch_verified_node_shasums_file_cached(http_client, &integrities_url, cache_dir)
            .await
            .map_err(NodeResolverError::FetchVerifiedNodeShasums)?
    } else {
        fetch_shasums_file_cached(http_client, &integrities_url, cache_dir)
            .await
            .map_err(NodeResolverError::FetchShasumsFile)?
    };
    let mut assets = Vec::new();
    for item in items {
        let Some(parsed) = parse_node_file_name(&item.file_name, version) else { continue };
        let is_musl = parsed.is_musl;
        if musl_only && !is_musl {
            continue;
        }
        let mut platform = parsed.platform;
        if platform == "win" {
            platform = "win32".to_string();
        }
        let libc = is_musl.then(|| "musl".to_string());
        let address = get_node_artifact_address(GetNodeArtifactAddressOptions {
            version,
            base_url: node_mirror_base_url,
            platform: &platform,
            arch: &parsed.arch,
            libc: libc.as_deref(),
        });
        let url = format!("{}/{}{}", address.dirname, address.basename, address.extname);
        let archive =
            if address.extname == ".zip" { BinaryArchive::Zip } else { BinaryArchive::Tarball };
        let integrity: Integrity =
            item.integrity.parse().map_err(|error| NodeResolverError::ParseIntegrity {
                integrity: item.integrity.clone(),
                file_name: item.file_name.clone(),
                error: Arc::new(error),
            })?;
        let prefix = matches!(archive, BinaryArchive::Zip).then(|| address.basename.clone());
        let binary = BinaryResolution {
            url,
            integrity,
            bin: bin_spec_for_platform(&platform),
            archive,
            prefix,
        };
        let target = PlatformAssetTarget { os: platform, cpu: parsed.arch, libc };
        assets.push(PlatformAssetResolution {
            resolution: LockfileResolution::Binary(binary),
            targets: vec![target],
        });
    }
    Ok(assets)
}

struct NodeFileName {
    platform: String,
    arch: String,
    is_musl: bool,
}

/// Match the nodejs.org artifact pattern
/// `^node-v<version>-([^-.]+)-([^.-]+)(-musl)?\.(tar\.gz|zip)$` —
/// implemented by hand so the resolver doesn't pay the regex crate
/// dependency for a single pattern.
fn parse_node_file_name(file_name: &str, version: &str) -> Option<NodeFileName> {
    let prefix = format!("node-v{version}-");
    let rest = file_name.strip_prefix(&prefix)?;
    let head = if let Some(head) = rest.strip_suffix(".tar.gz") {
        head
    } else {
        rest.strip_suffix(".zip")?
    };
    let (platform, after_platform) = head.split_once('-')?;
    if platform.is_empty() || platform.contains('.') {
        return None;
    }
    let (arch_part, is_musl) = match after_platform.strip_suffix("-musl") {
        Some(arch_part) => (arch_part, true),
        None => (after_platform, false),
    };
    if arch_part.is_empty() || arch_part.contains('.') || arch_part.contains('-') {
        return None;
    }
    Some(NodeFileName { platform: platform.to_string(), arch: arch_part.to_string(), is_musl })
}

fn bin_spec_for_platform(platform: &str) -> BinarySpec {
    let path = if platform == "win32" { "node.exe" } else { "bin/node" };
    BinarySpec::Map(BTreeMap::from([("node".to_string(), path.to_string())]))
}

fn node_bins_for_current_os(platform: &str) -> serde_json::Value {
    serde_json::json!({ "node": if platform == "win32" { "node.exe" } else { "bin/node" } })
}

/// Host platform string in pnpm's normalised form (`win32`, `darwin`,
/// `linux`, ...). Reads `std::env::consts::OS` rather than spawning a
/// helper so the lookup is allocation-free.
fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        other => other,
    }
}

#[cfg(test)]
mod tests;
