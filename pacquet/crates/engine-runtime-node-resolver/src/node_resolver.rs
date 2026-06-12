//! Pacquet port of upstream's
//! [`resolveNodeRuntime` / `resolveLatestNodeRuntime`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/index.ts#L39-L97).
//!
//! [`NodeResolver`] implements [`Resolver`] and ties the per-helper
//! pieces (parser, mirror picker, asset reader) together so the
//! default-resolver dispatcher can route `node@runtime:<spec>`
//! dependencies through it.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_crypto_shasums_file::{
    FetchShasumsFileError, FetchVerifiedNodeShasumsError, fetch_shasums_file,
    fetch_verified_node_shasums_file,
};
use pacquet_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PlatformAssetResolution,
    PlatformAssetTarget, VariationsResolution,
};
use pacquet_network::ThrottledClient;
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};
use ssri::Integrity;

use crate::{
    get_node_artifact_address::{GetNodeArtifactAddressOptions, get_node_artifact_address},
    get_node_mirror::{
        DEFAULT_NODE_MIRROR_BASE_URL, UNOFFICIAL_NODE_MIRROR_BASE_URL, get_node_mirror,
    },
    parse_node_specifier::{ParseNodeSpecifierError, parse_node_specifier},
    resolve_node_version::{ResolveNodeVersionError, resolve_node_version},
};

const RESOLVED_VIA: &str = "nodejs.org";
const BARE_SPEC_PREFIX: &str = "runtime:";

/// Errors emitted by [`NodeResolver::resolve`] / [`NodeResolver::resolve_latest`].
///
/// Each variant maps to one of upstream's `node-resolver` codes
/// (`NO_OFFLINE_NODEJS_RESOLUTION`, `NODEJS_VERSION_NOT_FOUND`,
/// `INVALID_NODE_RELEASE_CHANNEL`, plus the network failure modes
/// surfaced by the shasums-file and release-index fetchers).
#[derive(Debug, Display, Error, Diagnostic)]
pub enum NodeResolverError {
    #[display("Offline Node.js resolution is not supported")]
    #[diagnostic(code(NO_OFFLINE_NODEJS_RESOLUTION))]
    Offline,

    #[display("Could not find a Node.js version that satisfies {spec}")]
    #[diagnostic(code(NODEJS_VERSION_NOT_FOUND))]
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
    #[diagnostic(code(NODE_INTEGRITY_PARSE_FAILED))]
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
}

impl NodeResolver {
    #[must_use]
    pub fn new(http_client: Arc<ThrottledClient>) -> Self {
        Self { http_client, node_download_mirrors: HashMap::new(), offline: false }
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

        // Upstream's `currentPkg && !update` short-circuit reuses the
        // lockfile-pinned VariationsResolution unchanged. Pacquet
        // doesn't thread `currentPkg` through `ResolveOptions` yet, so
        // every resolve re-fetches the asset list. Restore the fast
        // path once the seam carries `currentPkg`.

        if self.offline {
            return Err(Box::new(NodeResolverError::Offline) as ResolveError);
        }

        let parsed = parse_node_specifier(version_spec).map_err(|err| {
            Box::new(NodeResolverError::InvalidReleaseChannel(err)) as ResolveError
        })?;
        let mirror = get_node_mirror(Some(&self.node_download_mirrors), &parsed.release_channel);
        let version =
            resolve_node_version(&self.http_client, &parsed.version_specifier, Some(&mirror))
                .await
                .map_err(|err| Box::new(NodeResolverError::FetchReleaseIndex(err)) as ResolveError)?
                .ok_or_else(|| {
                    Box::new(NodeResolverError::VersionNotFound { spec: version_spec.to_string() })
                        as ResolveError
                })?;
        let variants = self.read_node_assets(&mirror, &version, &parsed.release_channel).await?;
        let range = if version == version_spec { version.clone() } else { format!("^{version}") };
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
    /// Mirrors upstream's
    /// [`readNodeAssets`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/src/index.ts#L99-L113):
    /// the musl branch only fires when the active mirror is the
    /// default one (custom mirrors are assumed to publish their own
    /// musl-or-not policy), and musl-fetch failures are swallowed
    /// because old releases simply don't have musl builds.
    async fn read_node_assets(
        &self,
        mirror: &str,
        version: &str,
        release_channel: &str,
    ) -> Result<Vec<PlatformAssetResolution>, ResolveError> {
        let mut assets = read_node_assets_from_mirror(
            &self.http_client,
            mirror,
            version,
            /* musl_only */ false,
            /* verify_signature */ release_channel == "release",
        )
        .await?;
        if mirror == DEFAULT_NODE_MIRROR_BASE_URL
            && let Ok(mut musl_assets) = read_node_assets_from_mirror(
                &self.http_client,
                UNOFFICIAL_NODE_MIRROR_BASE_URL,
                version,
                /* musl_only */ true,
                /* verify_signature */ false,
            )
            .await
        {
            assets.append(&mut musl_assets);
        }
        Ok(assets)
    }
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

/// Read the asset list for one mirror version and decode each row
/// into a [`PlatformAssetResolution`].
///
/// The regex mirrors upstream's: `node-v<version>-<platform>-<arch>(-musl)?.(tar.gz|zip)`.
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
) -> Result<Vec<PlatformAssetResolution>, ResolveError> {
    let integrities_url = format!("{node_mirror_base_url}v{version}/SHASUMS256.txt");
    let items = if verify_signature {
        fetch_verified_node_shasums_file(http_client, &integrities_url).await.map_err(|err| {
            Box::new(NodeResolverError::FetchVerifiedNodeShasums(err)) as ResolveError
        })?
    } else {
        fetch_shasums_file(http_client, &integrities_url)
            .await
            .map_err(|err| Box::new(NodeResolverError::FetchShasumsFile(err)) as ResolveError)?
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
        let integrity: Integrity = item.integrity.parse().map_err(|error| {
            Box::new(NodeResolverError::ParseIntegrity {
                integrity: item.integrity.clone(),
                file_name: item.file_name.clone(),
                error: Arc::new(error),
            }) as ResolveError
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

/// Match upstream's
/// `^node-v<version>-([^-.]+)-([^.-]+)(-musl)?\.(tar\.gz|zip)$` —
/// implemented by hand so the resolver doesn't pay the regex crate
/// dependency for a single pattern. The version segment is matched
/// literally; the platform and arch each disallow `.` and `-`, and
/// `-musl` is the only legal third segment.
fn parse_node_file_name(file_name: &str, version: &str) -> Option<NodeFileName> {
    let prefix = format!("node-v{version}-");
    let rest = file_name.strip_prefix(&prefix)?;
    let (head, suffix) = if let Some(head) = rest.strip_suffix(".tar.gz") {
        (head, ".tar.gz")
    } else if let Some(head) = rest.strip_suffix(".zip") {
        (head, ".zip")
    } else {
        return None;
    };
    let _ = suffix;
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
    // pnpm records the runtime variant's `bin` as a named map keyed by the
    // executable name (`{ node: bin/node }`), not a bare string — mirror
    // that so the `variants[].resolution.bin` block round-trips.
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
