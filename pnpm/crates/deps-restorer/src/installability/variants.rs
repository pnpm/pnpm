//! A registry package version published with platform variants and no
//! archive that targets `any` installs only where one of its variants does
//! ([platform variants](https://github.com/pnpm/rfcs/blob/7908231210bd7012f826a635bbf8fbf3745fb8c4/pnpr/text/0000-platform-variants.md#install)):
//! a host none of them fits skips it as an optional dependency, as a
//! platform the manifest excludes would.

use crate::platform_selection::listed_link_platform;
use pnpm_lockfile::{
    LockfileResolution, PackageMetadata, PlatformAssetTarget, PlatformSelector,
    VariationsResolution,
};
use pnpm_package_is_installable::{
    InstallabilityError, InstallabilityOptions, Platform, UnsupportedPlatformError, WantedPlatform,
};

/// The variants of `metadata` when the version has no archive for a host
/// none of them fits: a registry package's, without an `any` variant.
pub(super) fn variants_only(metadata: &PackageMetadata) -> Option<&VariationsResolution> {
    match &metadata.resolution {
        LockfileResolution::Variations(variations)
            if !variations.is_runtime() && variations.any_variant().is_none() =>
        {
            Some(variations)
        }
        _ => None,
    }
}

/// Why `metadata`, the package `package_id`, cannot install under
/// `options`, when it is published with variants only and the installer
/// would find no archive for the platform it links: the host's, or the one
/// platform `supportedArchitectures` lists when it lists exactly one and
/// omits `current`. A variant fits as the installer selects
/// it, by its platform and by the floors and ABI the host meets; a
/// selection the installer finds ambiguous is left for it to report.
pub(super) fn variants_installability(
    package_id: &str,
    metadata: &PackageMetadata,
    options: &InstallabilityOptions<'_>,
) -> Option<InstallabilityError> {
    let variations = variants_only(metadata)?;
    let linked = match listed_link_platform(options.supported_architectures) {
        Some(listed) => listed,
        None => host_selector(options),
    };
    if !matches!(variations.select(&linked), Ok(None)) {
        return None;
    }
    let targets: Vec<&PlatformAssetTarget> = variations.variants
        .iter()
        .flat_map(|variant| &variant.targets)
        .collect();
    let current = Platform {
        os: vec![options.current_os.to_string()],
        cpu: vec![options.current_cpu.to_string()],
        libc: vec![options.current_libc.to_string()],
    };
    Some(InstallabilityError::Platform(UnsupportedPlatformError::new(
        package_id.to_string(),
        published_platforms(&targets),
        current,
    )))
}

/// The host `options` describe, as the installer selects a registry
/// package's variant for it: a C library pnpm could not read is none, and
/// the floors are probed when a variant has one.
fn host_selector(options: &InstallabilityOptions<'_>) -> PlatformSelector {
    PlatformSelector {
        os: options.current_os.to_string(),
        cpu: options.current_cpu.to_string(),
        libc: (options.current_libc != "unknown").then(|| options.current_libc.to_string()),
        floors: None,
    }
}

/// The C library a target runs on: its own, or glibc for a Linux target
/// that names none.
fn target_libc(target: &PlatformAssetTarget) -> Option<String> {
    target.libc
        .clone()
        .or_else(|| (target.os == "linux").then(|| "glibc".to_string()))
}

/// Every `os`, `cpu`, and Linux `libc` the variants were published for.
fn published_platforms(targets: &[&PlatformAssetTarget]) -> WantedPlatform {
    let mut os = Vec::new();
    let mut cpu = Vec::new();
    let mut libc = Vec::new();
    for target in targets
        .iter()
        .filter(|target| !target.wasm32 && !target.any)
    {
        push_unique(&mut os, &target.os);
        push_unique(&mut cpu, &target.cpu);
        if let Some(target_libc) = target_libc(target) {
            push_unique(&mut libc, &target_libc);
        }
    }
    WantedPlatform { os: Some(os), cpu: Some(cpu), libc: (!libc.is_empty()).then_some(libc) }
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values
        .iter()
        .any(|existing| existing == value)
    {
        values.push(value.to_string());
    }
}
