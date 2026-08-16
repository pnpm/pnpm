//! GitHub release reader for [`crate::YarnResolver`].
//!
//! `yarnpkg/zpm` publishes one zip per Rust target triple and no checksum
//! file, so both the version list and each asset's integrity come from the
//! releases API.

use std::sync::Arc;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_crypto_shasums_file::sha256_hex_to_sri;
use pnpm_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PlatformAssetResolution,
    PlatformAssetTarget,
};
use pnpm_network::ThrottledClient;
use serde::Deserialize;
use ssri::Integrity;

const RELEASES_URL: &str = "https://api.github.com/repos/yarnpkg/zpm/releases?per_page=100";

#[derive(Debug, Display, Error, Diagnostic)]
pub enum ReadYarnReleasesError {
    #[display("Failed to fetch the Yarn releases from {url}: {error}")]
    #[diagnostic(code(ERR_PNPM_YARN_RELEASES_FETCH))]
    Network {
        url: String,
        #[error(source)]
        error: Arc<reqwest::Error>,
    },

    #[display("Fetching the Yarn releases from {url} responded with status {status}")]
    #[diagnostic(
        code(ERR_PNPM_YARN_RELEASES_STATUS),
        help(
            "GitHub rate-limits anonymous requests. Wait for the limit to reset, or install Yarn 6 by hand."
        )
    )]
    StatusNotOk { url: String, status: u16 },

    #[display("Could not parse the Yarn releases from {url}: {error}")]
    #[diagnostic(code(ERR_PNPM_YARN_RELEASES_PARSE))]
    Parse {
        url: String,
        #[error(source)]
        error: Arc<serde_json::Error>,
    },

    #[display("The Yarn {version} release publishes no archive with a usable checksum")]
    #[diagnostic(code(ERR_PNPM_YARN_RELEASE_WITHOUT_ASSETS))]
    NoUsableAssets {
        #[error(not(source))]
        version: String,
    },

    #[display("Failed to parse integrity {integrity} for {file_name}")]
    #[diagnostic(code(ERR_PNPM_YARN_PARSE_INTEGRITY))]
    Integrity {
        integrity: String,
        file_name: String,
        #[error(source)]
        error: Arc<ssri::Error>,
    },
}

/// One published Yarn release: its version and the platform archives it
/// ships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YarnRelease {
    pub version: String,
    assets: Vec<YarnAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct YarnAsset {
    file_name: String,
    url: String,
    /// The asset's `sha256:<hex>` digest as the release API reports it.
    digest: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

/// Fetch the published Yarn releases, newest first.
pub async fn fetch_yarn_releases(
    http_client: &ThrottledClient,
) -> Result<Vec<YarnRelease>, ReadYarnReleasesError> {
    let response = http_client
        .acquire_for_url(RELEASES_URL)
        .await
        .get(RELEASES_URL)
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| ReadYarnReleasesError::Network {
            url: RELEASES_URL.to_string(),
            error: Arc::new(error),
        })?;
    if !response.status().is_success() {
        return Err(ReadYarnReleasesError::StatusNotOk {
            url: RELEASES_URL.to_string(),
            status: response.status().as_u16(),
        });
    }
    let body = response.text().await.map_err(|error| ReadYarnReleasesError::Network {
        url: RELEASES_URL.to_string(),
        error: Arc::new(error),
    })?;
    parse_releases(&body)
}

pub fn parse_releases(body: &str) -> Result<Vec<YarnRelease>, ReadYarnReleasesError> {
    let releases: Vec<GithubRelease> = serde_json::from_str(body).map_err(|error| {
        ReadYarnReleasesError::Parse { url: RELEASES_URL.to_string(), error: Arc::new(error) }
    })?;
    Ok(releases
        .into_iter()
        .filter_map(|release| {
            let version = release.tag_name.strip_prefix('v')?.to_string();
            let assets = release
                .assets
                .into_iter()
                .map(|asset| YarnAsset {
                    file_name: asset.name,
                    url: asset.browser_download_url,
                    digest: asset.digest,
                })
                .collect();
            Some(YarnRelease { version, assets })
        })
        .collect())
}

/// Decode a release's archives into the platform variants the lockfile
/// records.
pub fn asset_variants(
    release: &YarnRelease,
) -> Result<Vec<PlatformAssetResolution>, ReadYarnReleasesError> {
    // zpm ships Linux as a statically linked musl build and nothing else,
    // and a static musl binary runs on glibc hosts too. Recording it as
    // `libc: musl` would hide it from every glibc host, so the constraint
    // is only recorded once a release also ships a glibc build to choose
    // between.
    let has_glibc_build = release
        .assets
        .iter()
        .filter_map(|asset| {
            let target = parse_asset_name(&asset.file_name)?;
            // An asset the loop below skips is not a build to choose
            // between, so it cannot be what constrains the musl one.
            asset.digest.as_deref().and_then(sha256_digest_to_sri)?;
            Some(target)
        })
        .any(|target| target.os == "linux" && !target.musl);

    let mut variants = Vec::new();
    for asset in &release.assets {
        let Some(parsed) = parse_asset_name(&asset.file_name) else { continue };
        let Some(integrity) = asset.digest.as_deref().and_then(sha256_digest_to_sri) else {
            continue;
        };
        let integrity: Integrity =
            integrity.parse().map_err(|error| ReadYarnReleasesError::Integrity {
                integrity,
                file_name: asset.file_name.clone(),
                error: Arc::new(error),
            })?;
        let binary = BinaryResolution {
            url: asset.url.clone(),
            integrity,
            bin: BinarySpec::Single(yarn_bin_path(&parsed.os).to_string()),
            archive: BinaryArchive::Zip,
            // zpm's archives hold their files at the root, unlike the
            // runtime archives that wrap theirs in a versioned directory.
            prefix: None,
        };
        let target = PlatformAssetTarget {
            os: parsed.os,
            cpu: parsed.cpu,
            libc: (parsed.musl && has_glibc_build).then(|| "musl".to_string()),
        };
        variants.push(PlatformAssetResolution {
            resolution: LockfileResolution::Binary(binary),
            targets: vec![target],
        });
    }
    if variants.is_empty() {
        return Err(ReadYarnReleasesError::NoUsableAssets { version: release.version.clone() });
    }
    variants.sort_by(|left, right| variant_url(left).cmp(variant_url(right)));
    Ok(variants)
}

fn variant_url(variant: &PlatformAssetResolution) -> &str {
    match &variant.resolution {
        LockfileResolution::Binary(binary) => binary.url.as_str(),
        _ => "",
    }
}

/// The `sha256-<base64>` form of a release API `digest`, which is
/// `sha256:<hex>`. `None` for any other algorithm or a malformed digest —
/// such an asset is skipped rather than installed unverified.
fn sha256_digest_to_sri(digest: &str) -> Option<String> {
    sha256_hex_to_sri(digest.strip_prefix("sha256:")?)
}

struct YarnAssetTarget {
    os: String,
    cpu: String,
    musl: bool,
}

/// Decode `yarn-<target-triple>.zip` into the host triple it covers.
/// Unknown triples are skipped, so a new target pnpm has no mapping for
/// does not break the whole release.
fn parse_asset_name(file_name: &str) -> Option<YarnAssetTarget> {
    let triple = file_name.strip_suffix(".zip")?.strip_prefix("yarn-")?;
    let (arch, rest) = triple.split_once('-')?;
    let cpu = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "i686" => "ia32",
        "armv7" => "arm",
        _ => return None,
    };
    let (os, musl) = if rest.ends_with("apple-darwin") {
        ("darwin", false)
    } else if rest.ends_with("linux-musl") {
        ("linux", true)
    } else if rest.ends_with("linux-gnu") {
        ("linux", false)
    } else if rest.contains("windows") {
        ("win32", false)
    } else {
        return None;
    };
    Some(YarnAssetTarget { os: os.to_string(), cpu: cpu.to_string(), musl })
}

/// Yarn 6's archives carry two executables: `yarn`, a launcher that
/// re-dispatches to whatever version a project asks for (defaulting to
/// Yarn Classic when it finds no pin), and `yarn-bin`, Yarn 6 itself.
/// pnpm has already decided which version to run by the time it unpacks
/// the archive, so it links the engine, not the launcher.
fn yarn_bin_path(os: &str) -> &'static str {
    if os == "win32" { "yarn-bin.exe" } else { "yarn-bin" }
}

#[cfg(test)]
mod tests;
