use super::{
    Arc, AuthHeaders, BTreeMap, BinaryArchive, BinaryResolution, BinarySpec,
    GetNodeArtifactAddressOptions, Integrity, LockfileResolution, NodeResolverError, Path,
    PlatformAssetResolution, PlatformAssetTarget, ShasumsFileItem, ThrottledClient,
    fetch_shasums_file_cached, fetch_shasums_file_cached_with_auth_headers,
    fetch_verified_node_shasums_file_cached,
    fetch_verified_node_shasums_file_cached_with_auth_headers, get_node_artifact_address,
};

/// Read the asset list for one mirror version and decode each row
/// into a [`PlatformAssetResolution`].
///
/// Rows are matched against the nodejs.org artifact pattern
/// `node-v<version>-<platform>-<arch>(-musl)?.(tar.gz|zip)`.
/// Files that don't match (e.g. `.pkg`, `.msi`, source tarballs) are
/// dropped. When `musl_only` is true, glibc builds are filtered out
/// so the asset list only carries the musl-specific variants the
/// caller asked for.
pub(super) async fn read_node_assets_from_mirror(
    http_client: &ThrottledClient,
    auth_headers: &AuthHeaders,
    node_mirror_base_url: &str,
    version: &str,
    musl_only: bool,
    verify_signature: bool,
    cache_dir: Option<&Path>,
) -> Result<Vec<PlatformAssetResolution>, NodeResolverError> {
    // The URL is pinned to one released version, which is what makes it
    // eligible for the SHASUMS disk cache.
    let integrities_url = format!("{node_mirror_base_url}v{version}/SHASUMS256.txt");
    let items = fetch_node_shasums(ShasumsRequest {
        http_client,
        auth_headers,
        integrities_url: &integrities_url,
        cache_dir,
        verify_signature,
    })
    .await?;
    let mut assets = Vec::new();
    for item in items {
        let Some(parsed) = parse_node_file_name(&item.file_name, version) else { continue };
        if musl_only && !parsed.is_musl {
            continue;
        }
        assets.push(node_platform_asset(&item, parsed, version, node_mirror_base_url)?);
    }
    Ok(assets)
}

/// One request for a release's `SHASUMS256.txt`.
struct ShasumsRequest<'a> {
    http_client: &'a ThrottledClient,
    auth_headers: &'a AuthHeaders,
    integrities_url: &'a str,
    cache_dir: Option<&'a Path>,
    /// Whether the file's detached signature is checked against the release
    /// keys before its digests are trusted.
    verify_signature: bool,
}

/// Read a release's `SHASUMS256.txt`, through whichever of the four fetch
/// paths the signature and credential settings select.
async fn fetch_node_shasums(
    request: ShasumsRequest<'_>,
) -> Result<Vec<ShasumsFileItem>, NodeResolverError> {
    let ShasumsRequest { http_client, auth_headers, integrities_url, cache_dir, verify_signature } =
        request;
    match (verify_signature, auth_headers.is_empty()) {
        (true, true) => {
            fetch_verified_node_shasums_file_cached(http_client, integrities_url, cache_dir)
                .await
                .map_err(NodeResolverError::FetchVerifiedNodeShasums)
        }
        (true, false) => fetch_verified_node_shasums_file_cached_with_auth_headers(
            http_client,
            integrities_url,
            cache_dir,
            auth_headers,
        )
        .await
        .map_err(NodeResolverError::FetchVerifiedNodeShasums),
        (false, true) => fetch_shasums_file_cached(http_client, integrities_url, cache_dir)
            .await
            .map_err(NodeResolverError::FetchShasumsFile),
        (false, false) => fetch_shasums_file_cached_with_auth_headers(
            http_client,
            integrities_url,
            cache_dir,
            auth_headers,
        )
        .await
        .map_err(NodeResolverError::FetchShasumsFile),
    }
}

/// One platform's download, from the shasums entry that names it.
fn node_platform_asset(
    item: &ShasumsFileItem,
    parsed: NodeFileName,
    version: &str,
    node_mirror_base_url: &str,
) -> Result<PlatformAssetResolution, NodeResolverError> {
    let platform = if parsed.platform == "win" { "win32".to_string() } else { parsed.platform };
    let libc = parsed.is_musl.then(|| "musl".to_string());
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
    let binary =
        BinaryResolution { url, integrity, bin: bin_spec_for_platform(&platform), archive, prefix };
    let target = PlatformAssetTarget { os: platform, cpu: parsed.arch, libc };
    Ok(PlatformAssetResolution {
        resolution: LockfileResolution::Binary(binary),
        targets: vec![target],
    })
}

pub(super) struct NodeFileName {
    pub(super) platform: String,
    pub(super) arch: String,
    pub(super) is_musl: bool,
}

/// Match the nodejs.org artifact pattern
/// `^node-v<version>-([^-.]+)-([^.-]+)(-musl)?\.(tar\.gz|zip)$` —
/// implemented by hand so the resolver doesn't pay the regex crate
/// dependency for a single pattern.
pub(super) fn parse_node_file_name(file_name: &str, version: &str) -> Option<NodeFileName> {
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

pub(super) fn bin_spec_for_platform(platform: &str) -> BinarySpec {
    let path = if platform == "win32" { "node.exe" } else { "bin/node" };
    BinarySpec::Map(BTreeMap::from([("node".to_string(), path.to_string())]))
}

pub(super) fn node_bins_for_current_os(platform: &str) -> serde_json::Value {
    serde_json::json!({ "node": if platform == "win32" { "node.exe" } else { "bin/node" } })
}

/// Host platform string in pnpm's normalised form (`win32`, `darwin`,
/// `linux`, ...). Reads `std::env::consts::OS` rather than spawning a
/// helper so the lookup is allocation-free.
pub(super) fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        other => other,
    }
}
