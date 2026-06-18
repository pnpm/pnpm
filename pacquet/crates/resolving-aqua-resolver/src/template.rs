//! Pacquet port of upstream's
//! [`resolving/aqua-resolver/src/template.ts`](https://github.com/pnpm/pnpm/pull/10970)
//! (co-developed in the same PR).
//!
//! Expands the aqua-registry asset/checksum templates into concrete
//! per-platform download URLs. The template grammar is a small subset
//! of Go's `text/template` (`{{.Version}}`, `{{trimV .Version}}`,
//! `{{.OS}}`, `{{.Arch}}`, `{{.Format}}`, `{{.AssetWithoutExt}}`).

use crate::registry::{AquaFile, AquaOverride, Replacements, ResolvedSpec};

/// One `(os, cpu)` host triple, paired with the Go-flavored
/// `(goos, goarch)` tokens the registry templates expect.
struct TargetPlatform {
    os: &'static str,
    cpu: &'static str,
    goos: &'static str,
    goarch: &'static str,
}

const DEFAULT_PLATFORMS: &[TargetPlatform] = &[
    TargetPlatform { os: "darwin", cpu: "arm64", goos: "darwin", goarch: "arm64" },
    TargetPlatform { os: "darwin", cpu: "x64", goos: "darwin", goarch: "amd64" },
    TargetPlatform { os: "linux", cpu: "x64", goos: "linux", goarch: "amd64" },
    TargetPlatform { os: "linux", cpu: "arm64", goos: "linux", goarch: "arm64" },
    TargetPlatform { os: "win32", cpu: "x64", goos: "windows", goarch: "amd64" },
    TargetPlatform { os: "win32", cpu: "arm64", goos: "windows", goarch: "arm64" },
];

/// The `(os, cpu)` a [`ExpandedAsset`] targets.
pub struct ExpandedTarget {
    pub os: String,
    pub cpu: String,
}

/// The checksum-file template carried forward to the fetch step. The
/// algorithm is not retained: pnpm always emits `sha256-` integrity
/// regardless of the registry's declared algorithm.
pub struct ExpandedChecksum {
    pub asset: String,
}

/// A fully-expanded download for one platform.
pub struct ExpandedAsset {
    pub target: ExpandedTarget,
    pub url: String,
    pub asset_name: String,
    pub format: String,
    pub files: Vec<AquaFile>,
    pub checksum: Option<ExpandedChecksum>,
}

/// Expand `spec` into one [`ExpandedAsset`] per supported platform.
/// Empty when the spec has no `asset` template.
pub fn expand_assets(
    owner: &str,
    repo: &str,
    version: &str,
    spec: &ResolvedSpec<'_>,
) -> Vec<ExpandedAsset> {
    let Some(base_asset) = spec.asset else { return Vec::new() };

    let platforms = filter_supported_platforms(spec.supported_envs);
    let mut assets = Vec::new();

    for platform in platforms {
        let platform_override = find_platform_override(spec.overrides, platform);
        let format = platform_override
            .and_then(|over| over.format.as_deref())
            .or(spec.format)
            .unwrap_or("tar.gz");
        let replacements = merge_replacements(
            spec.replacements,
            platform_override.and_then(|over| over.replacements.as_ref()),
        );
        let asset_template =
            platform_override.and_then(|over| over.asset.as_deref()).unwrap_or(base_asset);
        let files = platform_override
            .and_then(|over| over.files.as_deref())
            .or(spec.files)
            .map_or_else(|| vec![AquaFile { name: repo.to_string(), src: None }], <[_]>::to_vec);

        let vars = build_template_vars(version, platform, format, &replacements);
        let asset_name = expand_template(asset_template, &vars);
        let url =
            format!("https://github.com/{owner}/{repo}/releases/download/{version}/{asset_name}");

        let checksum_config =
            platform_override.and_then(|over| over.checksum.as_ref()).or(spec.checksum);
        let checksum = checksum_config.and_then(|config| {
            if config.enabled == Some(false) {
                return None;
            }
            match (config.asset.as_deref(), config.checksum_type.as_deref()) {
                (Some(asset), Some(_)) => Some(ExpandedChecksum { asset: asset.to_string() }),
                _ => None,
            }
        });

        assets.push(ExpandedAsset {
            target: ExpandedTarget { os: platform.os.to_string(), cpu: platform.cpu.to_string() },
            url,
            asset_name,
            format: format.to_string(),
            files: files
                .into_iter()
                .map(|file| AquaFile {
                    name: file.name,
                    src: file.src.map(|src| expand_template(&src, &vars)),
                })
                .collect(),
            checksum,
        });
    }

    assets
}

fn filter_supported_platforms(supported_envs: Option<&[String]>) -> Vec<&'static TargetPlatform> {
    let Some(supported_envs) = supported_envs else {
        return DEFAULT_PLATFORMS.iter().collect();
    };
    DEFAULT_PLATFORMS
        .iter()
        .filter(|platform| {
            supported_envs.iter().any(|env| match env.split_once('/') {
                Some((goos, goarch)) => goos == platform.goos && goarch == platform.goarch,
                None => env == platform.goos || env == platform.goarch,
            })
        })
        .collect()
}

/// Find the most specific `overrides:` entry for `platform`: an entry
/// matching both `goos` and `goarch` wins outright, else the first
/// `goos`-only match.
fn find_platform_override<'a>(
    overrides: Option<&'a [AquaOverride]>,
    platform: &TargetPlatform,
) -> Option<&'a AquaOverride> {
    let overrides = overrides?;
    let mut goos_match: Option<&AquaOverride> = None;
    for over in overrides {
        let goos_matches = over.goos.as_deref().is_none_or(|goos| goos == platform.goos);
        let goarch_matches = over.goarch.as_deref().is_none_or(|goarch| goarch == platform.goarch);
        if goos_matches && goarch_matches {
            if over.goos.is_some() && over.goarch.is_some() {
                return Some(over);
            }
            if over.goos.is_some() {
                goos_match = Some(over);
            }
        }
    }
    goos_match
}

fn merge_replacements(base: Option<&Replacements>, over: Option<&Replacements>) -> Replacements {
    let mut merged = base.cloned().unwrap_or_default();
    if let Some(over) = over {
        for (key, value) in over {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

struct TemplateVars {
    version: String,
    trimmed_version: String,
    os: String,
    arch: String,
    format: String,
}

fn build_template_vars(
    version: &str,
    platform: &TargetPlatform,
    format: &str,
    replacements: &Replacements,
) -> TemplateVars {
    let os = replacements.get(platform.goos).map_or(platform.goos, String::as_str);
    let arch = replacements.get(platform.goarch).map_or(platform.goarch, String::as_str);
    TemplateVars {
        version: version.to_string(),
        trimmed_version: version.strip_prefix('v').unwrap_or(version).to_string(),
        os: os.to_string(),
        arch: arch.to_string(),
        format: format.to_string(),
    }
}

fn expand_template(template: &str, vars: &TemplateVars) -> String {
    let mut result = template
        .replace("{{.Version}}", &vars.version)
        .replace("{{trimV .Version}}", &vars.trimmed_version)
        .replace("{{.Arch}}", &vars.arch)
        .replace("{{.OS}}", &vars.os)
        .replace("{{.Format}}", &vars.format);
    if result.contains("{{.AssetWithoutExt}}") {
        let asset_full = expand_template(&template.replace("{{.AssetWithoutExt}}", ""), vars);
        let without_ext = strip_format_extension(&asset_full, &vars.format);
        result = result.replace("{{.AssetWithoutExt}}", &without_ext);
    }
    result
}

fn strip_format_extension(name: &str, format: &str) -> String {
    name.strip_suffix(&format!(".{format}")).unwrap_or(name).to_string()
}

/// Expand the `checksum.asset` template, which references the resolved
/// asset name (`{{.Asset}}`) and version rather than platform tokens.
pub fn expand_checksum_asset_name(
    checksum_template: &str,
    asset_name: &str,
    version: &str,
) -> String {
    let trimmed_version = version.strip_prefix('v').unwrap_or(version);
    checksum_template
        .replace("{{.Asset}}", asset_name)
        .replace("{{.Version}}", version)
        .replace("{{trimV .Version}}", trimmed_version)
}

#[cfg(test)]
mod tests;
