//! A registry package version published with platform variants and no
//! archive that targets `any` installs only where one of its variants does
//! ([platform variants](https://github.com/pnpm/rfcs/blob/7908231210bd7012f826a635bbf8fbf3745fb8c4/pnpr/text/0000-platform-variants.md#install)):
//! a host none of them fits skips it as an optional dependency, as a
//! platform the manifest excludes would.

use pnpm_lockfile::{
    LockfileResolution, PackageMetadata, PlatformAssetTarget, VariationsResolution,
};
use pnpm_package_is_installable::{
    InstallabilityError, InstallabilityOptions, Platform, UnsupportedPlatformError, WantedPlatform,
    WantedPlatformRef, platform_is_supported,
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
/// `options`, when it is published with variants only and none of them fits
/// the host. The installer links the host's variant, so a platform that only
/// `supportedArchitectures` names does not make the package installable. A
/// variant fits by its `os`, `cpu`, and Linux `libc`, whatever its floors.
pub(super) fn variants_installability(
    package_id: &str,
    metadata: &PackageMetadata,
    options: &InstallabilityOptions<'_>,
) -> Option<InstallabilityError> {
    let variations = variants_only(metadata)?;
    let targets: Vec<&PlatformAssetTarget> = variations.variants
        .iter()
        .flat_map(|variant| &variant.targets)
        .collect();
    if targets.iter().any(|target| target_fits(target, options)) {
        return None;
    }
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

/// Whether the host can run an archive built for `target`.
fn target_fits(target: &PlatformAssetTarget, options: &InstallabilityOptions<'_>) -> bool {
    if target.wasm32 {
        return true;
    }
    let os = [target.os.clone()];
    let cpu = [target.cpu.clone()];
    let libc = target_libc(target).map(|libc| [libc]);
    platform_is_supported(
        WantedPlatformRef {
            os: Some(&os),
            cpu: Some(&cpu),
            libc: libc.as_ref().map(|libc| &libc[..]),
        },
        None,
        options.current_os,
        options.current_cpu,
        options.current_libc,
    )
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
