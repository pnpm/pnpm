//! Pacquet port of pnpm's `@pnpm/resolving.aqua-resolver`
//! ([pnpm/pnpm#10970](https://github.com/pnpm/pnpm/pull/10970),
//! co-developed in the same PR).
//!
//! Resolves `aqua:owner/repo[@version]` dependencies into a
//! [`VariationsResolution`] of per-platform [`BinaryResolution`]s,
//! sourcing prebuilt CLI binaries from GitHub Releases through the
//! [aqua-registry](https://github.com/aquaproj/aqua-registry). The
//! resolver fetches the registry package definition and the release tag
//! in parallel, expands the registry's asset templates for every
//! supported platform, and attaches per-asset integrity pulled from the
//! release's checksum files.

mod error;
mod github;
mod registry;
mod template;

use std::{collections::BTreeMap, sync::Arc};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use pacquet_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PlatformAssetResolution,
    PlatformAssetTarget, VariationsResolution,
};
use pacquet_network::ThrottledClient;
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, UpdateBehavior, WantedDependency,
};
use ssri::Integrity;

pub use error::AquaResolverError;

use crate::{
    github::{fetch_checksum_file, resolve_github_version},
    registry::{fetch_aqua_registry_package, find_matching_override},
    template::{ExpandedAsset, expand_assets, expand_checksum_asset_name},
};

const BARE_SPEC_PREFIX: &str = "aqua:";
const RESOLVED_VIA: &str = "aqua";

/// Aqua resolver entry point. Owns the throttled HTTP client used for
/// the registry, GitHub API, and checksum-file requests.
pub struct AquaResolver {
    pub http_client: Arc<ThrottledClient>,
    pub offline: bool,
}

impl AquaResolver {
    #[must_use]
    pub fn new(http_client: Arc<ThrottledClient>) -> Self {
        Self { http_client, offline: false }
    }
}

impl Resolver for AquaResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(self.resolve_impl(wanted_dependency, opts))
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        // Aqua dependencies are pinned by tag; upstream does not register
        // the aqua resolver in the `resolveLatest` chain, so there is no
        // "latest" notion to report. Decline so the dispatcher falls
        // through.
        Box::pin(async { Ok(None) })
    }
}

impl AquaResolver {
    async fn resolve_impl(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(specifier) = wanted_dependency.bare_specifier.as_deref() else {
            return Ok(None);
        };
        if !specifier.starts_with(BARE_SPEC_PREFIX) {
            return Ok(None);
        }

        if self.offline {
            return Err(Box::new(AquaResolverError::Offline) as ResolveError);
        }

        if opts.update == UpdateBehavior::Off
            && let Some(current_pkg) = opts.current_pkg.as_ref()
        {
            return Ok(Some(ResolveResult {
                id: current_pkg.id.clone(),
                name_ver: None,
                latest: None,
                published_at: None,
                manifest: None,
                resolution: current_pkg.resolution.clone(),
                resolved_via: RESOLVED_VIA.to_string(),
                normalized_bare_specifier: None,
                alias: wanted_dependency.alias.clone(),
                policy_violation: None,
            }));
        }

        let ParsedSpecifier { owner, repo, version_spec } = parse_aqua_specifier(specifier)?;

        // Fetch the registry definition and resolve the release tag in
        // parallel, mirroring upstream's `Promise.all`.
        let registry_future = fetch_aqua_registry_package(&self.http_client, &owner, &repo);
        let version_future =
            resolve_github_version(&self.http_client, &owner, &repo, version_spec.as_deref());
        let (pkg, version) = tokio::try_join!(registry_future, version_future)?;

        let spec = find_matching_override(&pkg, &version);
        let expanded_assets = expand_assets(&owner, &repo, &version, &spec);
        if expanded_assets.is_empty() {
            return Err(Box::new(AquaResolverError::NoAssets {
                owner: owner.clone(),
                repo: repo.clone(),
                version: version.clone(),
            }) as ResolveError);
        }

        let checksums_by_asset =
            resolve_checksums(&self.http_client, &owner, &repo, &version, &expanded_assets).await?;

        let clean_version = version.strip_prefix('v').unwrap_or(&version).to_string();
        let mut variants = Vec::with_capacity(expanded_assets.len());
        for expanded in &expanded_assets {
            let integrity = match checksums_by_asset.get(&expanded.asset_name) {
                Some(sha256_hex) => integrity_from_hex(&expanded.asset_name, sha256_hex)?,
                None => Integrity { hashes: Vec::new() },
            };
            let binary = BinaryResolution {
                url: expanded.url.clone(),
                integrity,
                bin: derive_bin(&expanded.files, &expanded.format),
                archive: if expanded.format == "zip" {
                    BinaryArchive::Zip
                } else {
                    BinaryArchive::Tarball
                },
                prefix: None,
            };
            variants.push(PlatformAssetResolution {
                resolution: LockfileResolution::Binary(binary),
                targets: vec![PlatformAssetTarget {
                    os: expanded.target.os.clone(),
                    cpu: expanded.target.cpu.clone(),
                    libc: None,
                }],
            });
        }
        variants.sort_by(|a, b| variant_url(a).cmp(variant_url(b)));

        let pkg_name = repo.to_lowercase();
        let manifest = serde_json::json!({
            "name": pkg_name,
            "version": clean_version,
        });
        Ok(Some(ResolveResult {
            id: format!("{pkg_name}@aqua:{clean_version}").into(),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(Arc::new(manifest)),
            resolution: LockfileResolution::Variations(VariationsResolution { variants }),
            resolved_via: RESOLVED_VIA.to_string(),
            normalized_bare_specifier: Some(format!("aqua:{owner}/{repo}@{clean_version}")),
            alias: wanted_dependency.alias.clone().or(Some(pkg_name)),
            policy_violation: None,
        }))
    }
}

struct ParsedSpecifier {
    owner: String,
    repo: String,
    version_spec: Option<String>,
}

fn parse_aqua_specifier(specifier: &str) -> Result<ParsedSpecifier, AquaResolverError> {
    let invalid = || AquaResolverError::InvalidSpecifier { specifier: specifier.to_string() };
    let rest = &specifier[BARE_SPEC_PREFIX.len()..];

    let (owner_repo, version_spec) = match rest.rfind('@') {
        Some(idx) if idx > 0 => (&rest[..idx], Some(rest[idx + 1..].to_string())),
        _ => (rest, None),
    };

    let slash_idx = owner_repo.find('/').filter(|&idx| idx > 0).ok_or_else(invalid)?;
    Ok(ParsedSpecifier {
        owner: owner_repo[..slash_idx].to_string(),
        repo: owner_repo[slash_idx + 1..].to_string(),
        version_spec,
    })
}

/// Fetch the checksum files for every asset that declares one, then map
/// each asset name to its hex SHA-256. Checksum files are de-duplicated
/// so a shared `_checksums.txt` is fetched once.
async fn resolve_checksums(
    http_client: &ThrottledClient,
    owner: &str,
    repo: &str,
    version: &str,
    expanded_assets: &[ExpandedAsset],
) -> Result<BTreeMap<String, String>, AquaResolverError> {
    let mut checksum_files: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for asset in expanded_assets {
        let Some(checksum) = &asset.checksum else { continue };
        let checksum_asset_name =
            expand_checksum_asset_name(&checksum.asset, &asset.asset_name, version);
        checksum_files.entry(checksum_asset_name).or_default().push(asset.asset_name.clone());
    }

    let mut result = BTreeMap::new();
    for (checksum_file_name, asset_names) in checksum_files {
        let checksums =
            fetch_checksum_file(http_client, owner, repo, version, &checksum_file_name).await?;
        for asset_name in asset_names {
            // Exact match first, then the single-hash fallback (a
            // per-asset `.sha256` sidecar stores one bare hash).
            if let Some(hash) = checksums.get(&asset_name).or_else(|| checksums.get("")) {
                result.insert(asset_name, hash.clone());
            }
        }
    }
    Ok(result)
}

fn variant_url(variant: &PlatformAssetResolution) -> &str {
    match &variant.resolution {
        LockfileResolution::Binary(binary) => binary.url.as_str(),
        _ => "",
    }
}

fn integrity_from_hex(asset_name: &str, sha256_hex: &str) -> Result<Integrity, AquaResolverError> {
    let bytes = decode_hex(sha256_hex).unwrap_or_default();
    let sri = format!("sha256-{}", BASE64_STANDARD.encode(bytes));
    sri.parse().map_err(|error| AquaResolverError::Integrity {
        asset_name: asset_name.to_string(),
        integrity: sri.clone(),
        error: Arc::new(error),
    })
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok())
        .collect()
}

/// Derive the `bin` field from the registry's `files` list. A single
/// file collapses to a bare path; multiple files become a
/// `name -> path` map. Tarball entries have their leading directory
/// component stripped to match pnpm's tarball extraction.
fn derive_bin(files: &[crate::registry::AquaFile], format: &str) -> BinarySpec {
    match files {
        [] => BinarySpec::Single(String::new()),
        [file] => BinarySpec::Single(match &file.src {
            Some(src) => strip_first_path_component(src, format),
            None => file.name.clone(),
        }),
        files => BinarySpec::Map(
            files
                .iter()
                .map(|file| {
                    let path = match &file.src {
                        Some(src) => strip_first_path_component(src, format),
                        None => file.name.clone(),
                    };
                    (file.name.clone(), path)
                })
                .collect(),
        ),
    }
}

fn strip_first_path_component(file_path: &str, format: &str) -> String {
    if !matches!(format, "tar.gz" | "tgz" | "tar.bz2" | "tar.xz") {
        return file_path.to_string();
    }
    match file_path.split_once('/') {
        Some((_, rest)) => rest.to_string(),
        None => file_path.to_string(),
    }
}

#[cfg(test)]
mod tests;
