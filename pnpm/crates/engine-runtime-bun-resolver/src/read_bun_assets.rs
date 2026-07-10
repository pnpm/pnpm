//! GitHub Release SHASUMS reader for [`crate::BunResolver`].
//!
//! Bun's release assets share one `SHASUMS256.txt` at
//! `https://github.com/oven-sh/bun/releases/download/bun-v<version>/SHASUMS256.txt`.
//! Each row covers one platform variant — pacquet decodes them in one
//! pass, sorts the result by URL, and emits
//! [`PlatformAssetResolution`] entries
//! the lockfile records.

use std::sync::Arc;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_crypto_shasums_file::{FetchShasumsFileError, fetch_shasums_file};
use pacquet_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PlatformAssetResolution,
    PlatformAssetTarget,
};
use pacquet_network::ThrottledClient;
use ssri::Integrity;

#[derive(Debug, Display, Error, Diagnostic)]
pub enum ReadBunAssetsError {
    #[diagnostic(transparent)]
    FetchShasumsFile(#[error(source)] FetchShasumsFileError),

    #[display("Failed to parse integrity {integrity} for {file_name}")]
    #[diagnostic(code(BUN_PARSE_INTEGRITY))]
    Integrity {
        integrity: String,
        file_name: String,
        #[error(source)]
        error: Arc<ssri::Error>,
    },
}

/// Fetch and decode the Bun-release SHASUMS file for `version`.
pub async fn read_bun_assets(
    http_client: &ThrottledClient,
    version: &str,
) -> Result<Vec<PlatformAssetResolution>, ReadBunAssetsError> {
    let integrities_url =
        format!("https://github.com/oven-sh/bun/releases/download/bun-v{version}/SHASUMS256.txt");
    let items = fetch_shasums_file(http_client, &integrities_url)
        .await
        .map_err(ReadBunAssetsError::FetchShasumsFile)?;

    let mut variants = Vec::new();
    for item in items {
        let Some(parsed) = parse_asset_name(&item.file_name) else { continue };
        let integrity: Integrity =
            item.integrity.parse().map_err(|error| ReadBunAssetsError::Integrity {
                integrity: item.integrity.clone(),
                file_name: item.file_name.clone(),
                error: Arc::new(error),
            })?;
        let url = format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{version}/{file_name}",
            file_name = item.file_name,
        );
        let prefix = item.file_name.strip_suffix(".zip").map(str::to_string);
        let binary = BinaryResolution {
            url,
            integrity,
            bin: BinarySpec::Single(bun_bin_path(&parsed.platform).to_string()),
            archive: BinaryArchive::Zip,
            prefix,
        };
        let target = PlatformAssetTarget {
            os: parsed.platform,
            cpu: parsed.arch,
            libc: parsed.musl.then(|| "musl".to_string()),
        };
        variants.push(PlatformAssetResolution {
            resolution: LockfileResolution::Binary(binary),
            targets: vec![target],
        });
    }
    variants.sort_by(|a, b| variant_url(a).cmp(variant_url(b)));
    Ok(variants)
}

fn variant_url(variant: &PlatformAssetResolution) -> &str {
    match &variant.resolution {
        LockfileResolution::Binary(binary) => binary.url.as_str(),
        _ => "",
    }
}

struct BunAssetName {
    platform: String,
    arch: String,
    musl: bool,
}

/// Match upstream's `^bun-([^-.]+)-([^-.]+)(-musl)?\.zip$` regex.
fn parse_asset_name(file_name: &str) -> Option<BunAssetName> {
    let stem = file_name.strip_suffix(".zip")?;
    let body = stem.strip_prefix("bun-")?;
    let (platform_raw, after_platform) = body.split_once('-')?;
    if platform_raw.is_empty() || platform_raw.contains('.') {
        return None;
    }
    let (arch_raw, musl) = match after_platform.strip_suffix("-musl") {
        Some(arch_raw) => (arch_raw, true),
        None => (after_platform, false),
    };
    if arch_raw.is_empty() || arch_raw.contains('.') || arch_raw.contains('-') {
        return None;
    }
    let platform = if platform_raw == "windows" { "win32" } else { platform_raw };
    let arch = if arch_raw == "aarch64" { "arm64" } else { arch_raw };
    Some(BunAssetName { platform: platform.to_string(), arch: arch.to_string(), musl })
}

fn bun_bin_path(os: &str) -> &'static str {
    if os == "win32" { "bun.exe" } else { "bun" }
}

#[cfg(test)]
mod tests;
