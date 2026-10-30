use super::{
    LockfileResolution, MissingTarballUrlError, PackageVersion, ResolveError, TarballResolution,
    redact_and_sanitize,
    resolution_result::{RegistryResolutionSource, normalize_tarball_url},
};
use pnpm_lockfile::{PlatformAssetResolution, PlatformAssetTarget, VariationsResolution};
use pnpm_registry::PnprVariant;

/// Wrap the primary `resolution` in a `variations` resolution when the
/// version publishes platform variants under `dist.pnpr.variants`. The
/// primary becomes the fallback for a host no variant matches.
pub(super) fn with_platform_variants(
    resolution: LockfileResolution,
    picked: &PackageVersion,
    source: &RegistryResolutionSource<'_>,
) -> Result<LockfileResolution, ResolveError> {
    let Some(variants) = picked.dist.pnpr.as_ref().and_then(|pnpr| pnpr.variants.as_ref()) else {
        return Ok(resolution);
    };
    if variants.is_empty() {
        return Ok(resolution);
    }
    let variants = variants
        .iter()
        .map(|variant| variant_resolution(variant, picked, source))
        .collect::<Result<_, _>>()?;
    Ok(LockfileResolution::Variations(VariationsResolution {
        variants,
        fallback: Some(Box::new(resolution)),
    }))
}

fn variant_resolution(
    variant: &PnprVariant,
    picked: &PackageVersion,
    source: &RegistryResolutionSource<'_>,
) -> Result<PlatformAssetResolution, ResolveError> {
    let version = picked.version.to_string();
    let invalid = || {
        Box::new(MissingTarballUrlError {
            package: format!("{}@{version} variant {}", picked.name, variant.tag),
            registry: redact_and_sanitize(source.registry),
        }) as ResolveError
    };
    let integrity = variant.integrity.parse().map_err(|_| invalid())?;
    let tarball = match &variant.tarball {
        Some(tarball) => normalize_tarball_url(tarball),
        None => pnpm_lockfile::variant_registry_tarball_url(
            &picked.name,
            &version,
            &variant.tag,
            source.registry,
        )
        .ok_or_else(invalid)?,
    };
    let targets = variant.platforms
        .iter()
        .map(|platform| PlatformAssetTarget {
            os: platform.os.clone(),
            cpu: platform.cpu.clone(),
            // The lockfile spells glibc as an absent `libc`.
            libc: platform.libc
                .clone()
                .filter(|libc| libc != "glibc"),
        })
        .collect();
    Ok(PlatformAssetResolution {
        tag: Some(variant.tag.clone()),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball,
            integrity: Some(integrity),
            revision: None,
            git_hosted: None,
            path: None,
        }),
        targets,
    })
}
