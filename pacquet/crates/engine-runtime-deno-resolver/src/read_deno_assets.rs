//! GitHub Releases asset enumerator for [`crate::DenoResolver`].
//!
//! Each Deno release publishes a `deno-<arch>-<vendor>-<os>.zip`
//! per platform plus a sibling `<asset>.sha256sum` text file with
//! the hex-encoded SHA-256. This module:
//!
//! 1. GETs `https://api.github.com/repos/denoland/deno/releases/tags/v<version>`
//!    and pulls the `assets[]` list.
//! 2. For each asset whose filename matches the deno-release pattern,
//!    fetches the sibling `.sha256sum`, decodes the hex hash, and
//!    builds a [`PlatformAssetResolution`].
//! 3. Sorts the resulting variants lexically by URL — same as upstream's
//!    [`lexCompare`](https://github.com/pnpm/util.lex-comparator/blob/main/src/index.ts)
//!    — so the lockfile stays diff-stable across runs.

use std::sync::Arc;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PlatformAssetResolution,
    PlatformAssetTarget,
};
use pacquet_network::ThrottledClient;
use serde::Deserialize;
use ssri::Integrity;

/// Errors raised by the asset enumerator. Surfaced through
/// [`DenoResolverError::ReadAssets`](crate::DenoResolverError::ReadAssets).
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ReadDenoAssetsError {
    #[display("No assets found for Deno v{version}")]
    #[diagnostic(code(DENO_MISSING_ASSETS))]
    MissingAssets {
        #[error(not(source))]
        version: String,
    },

    #[display("Failed to GET sha256 at {url}")]
    #[diagnostic(code(DENO_GITHUB_FAILURE))]
    GithubFailure {
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("Failed to GET sha256 at {url} (status: {status})")]
    #[diagnostic(code(DENO_GITHUB_FAILURE))]
    GithubStatus { url: String, status: u16 },

    #[display("No SHA256 in {url}")]
    #[diagnostic(code(DENO_PARSE_HASH))]
    ParseHash {
        #[error(not(source))]
        url: String,
    },

    #[display("Failed to GET release index for Deno v{version}")]
    #[diagnostic(code(DENO_GITHUB_FAILURE))]
    FetchReleaseIndex {
        version: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("Failed to decode release index for Deno v{version}")]
    #[diagnostic(code(DENO_GITHUB_FAILURE))]
    DecodeReleaseIndex {
        version: String,
        #[error(source)]
        error: Arc<serde_json::Error>,
    },

    #[display("Failed to parse integrity for {url}")]
    #[diagnostic(code(DENO_PARSE_HASH))]
    Integrity {
        url: String,
        #[error(source)]
        error: Arc<ssri::Error>,
    },
}

#[derive(Deserialize)]
struct ReleaseIndex {
    #[serde(default)]
    assets: Option<Vec<ReleaseAsset>>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

/// Fetch and decode every Deno release asset for `version`.
pub async fn read_deno_assets(
    http_client: &ThrottledClient,
    version: &str,
) -> Result<Vec<PlatformAssetResolution>, ReadDenoAssetsError> {
    let release_index_url =
        format!("https://api.github.com/repos/denoland/deno/releases/tags/v{version}");
    let response = http_client
        .acquire_for_url(&release_index_url)
        .await
        .get(&release_index_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| ReadDenoAssetsError::FetchReleaseIndex {
            version: version.to_string(),
            error: Arc::new(error),
        })?;
    let body = response.text().await.map_err(|error| ReadDenoAssetsError::FetchReleaseIndex {
        version: version.to_string(),
        error: Arc::new(error),
    })?;
    let index: ReleaseIndex =
        serde_json::from_str(&body).map_err(|error| ReadDenoAssetsError::DecodeReleaseIndex {
            version: version.to_string(),
            error: Arc::new(error),
        })?;
    let assets = index
        .assets
        .ok_or_else(|| ReadDenoAssetsError::MissingAssets { version: version.to_string() })?;

    let mut variants = Vec::new();
    for asset in &assets {
        let Some(targets) = parse_asset_name(&asset.name) else { continue };
        let sha256 = fetch_sha256(http_client, &asset.browser_download_url).await?;
        // `fetch_sha256` already validates that `sha256` is a 64-char
        // lower-case hex run via `extract_sha256`, so `decode_hex`
        // cannot fail here. Map the impossible-failure branch to
        // `DENO_PARSE_HASH` rather than silently falling back to an
        // empty byte slice so a future change to `extract_sha256`
        // that loosens the validator surfaces with the right error
        // code instead of an opaque integrity-parse failure.
        let hex_bytes = decode_hex(&sha256).ok_or_else(|| ReadDenoAssetsError::ParseHash {
            url: asset.browser_download_url.clone(),
        })?;
        let integrity_string = format!("sha256-{}", BASE64_STANDARD.encode(hex_bytes));
        let integrity: Integrity =
            integrity_string.parse().map_err(|error| ReadDenoAssetsError::Integrity {
                url: asset.browser_download_url.clone(),
                error: Arc::new(error),
            })?;
        let archive_url = asset
            .browser_download_url
            .strip_suffix(".sha256sum")
            .unwrap_or(&asset.browser_download_url)
            .to_string();
        let binary = BinaryResolution {
            url: archive_url,
            integrity,
            bin: BinarySpec::Single(deno_bin_path(&targets[0].os).to_string()),
            archive: BinaryArchive::Zip,
            prefix: None,
        };
        variants.push(PlatformAssetResolution {
            resolution: LockfileResolution::Binary(binary),
            targets,
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

/// Parse `deno-<cpu>-<vendor-os>.zip.sha256sum` into the
/// [`PlatformAssetTarget`]s the variant covers.
///
/// Two architectures (`aarch64`, `x86_64`) × three vendor-os pairs
/// (`apple-darwin`, `unknown-linux-gnu`, `pc-windows-msvc`) — anything
/// else falls through unmatched. Windows x64 also lists `arm64` in
/// its targets because the Windows x64 build runs natively on arm64
/// hosts under emulation.
fn parse_asset_name(name: &str) -> Option<Vec<PlatformAssetTarget>> {
    let stem = name.strip_suffix(".zip.sha256sum")?;
    let body = stem.strip_prefix("deno-")?;
    let (cpu_raw, vendor_os) = body.split_once('-')?;
    let cpu = match cpu_raw {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => return None,
    };
    let os = match vendor_os {
        "apple-darwin" => "darwin",
        "unknown-linux-gnu" => "linux",
        "pc-windows-msvc" => "win32",
        _ => return None,
    };
    let mut targets =
        vec![PlatformAssetTarget { os: os.to_string(), cpu: cpu.to_string(), libc: None }];
    if os == "win32" && cpu == "x64" {
        targets.push(PlatformAssetTarget {
            os: "win32".to_string(),
            cpu: "arm64".to_string(),
            libc: None,
        });
    }
    Some(targets)
}

async fn fetch_sha256(
    http_client: &ThrottledClient,
    url: &str,
) -> Result<String, ReadDenoAssetsError> {
    let response =
        http_client.acquire_for_url(url).await.get(url).send().await.map_err(|error| {
            ReadDenoAssetsError::GithubFailure { url: url.to_string(), error: Arc::new(error) }
        })?;
    if !response.status().is_success() {
        return Err(ReadDenoAssetsError::GithubStatus {
            url: url.to_string(),
            status: response.status().as_u16(),
        });
    }
    let body = response.text().await.map_err(|error| ReadDenoAssetsError::GithubFailure {
        url: url.to_string(),
        error: Arc::new(error),
    })?;
    extract_sha256(&body).ok_or_else(|| ReadDenoAssetsError::ParseHash { url: url.to_string() })
}

/// Lift a 64-character hex string out of an arbitrary body. Mirrors
/// upstream's `txt.match(/([a-f0-9]{64})/i)` regex.
fn extract_sha256(body: &str) -> Option<String> {
    let bytes = body.as_bytes();
    bytes
        .windows(64)
        .find(|window| window.iter().all(u8::is_ascii_hexdigit))
        .map(|window| String::from_utf8_lossy(window).to_ascii_lowercase())
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

fn deno_bin_path(os: &str) -> &'static str {
    if os == "win32" { "deno.exe" } else { "deno" }
}

#[cfg(test)]
mod tests;
