use super::{
    LockfileResolution, MissingTarballUrlError, PackageVersion, ResolveError, TarballResolution,
    redact_and_sanitize, resolution_result::RegistryResolutionSource,
};
use pnpm_lockfile::{
    PlatformAssetResolution, PlatformAssetTarget, TarballRevision, VariationsResolution,
};
use pnpm_platform_variants::{Platform, variant_tag};
use pnpm_registry::PnprVariant;

/// Wrap the primary `resolution` in a `variations` resolution when pnpr's
/// document lists platform variants for the version. The primary becomes
/// the fallback for a host no variant matches. The variants are written in
/// the byte order of their tags: the installer ranks them for the host, so
/// their order chooses nothing and only has to be the same everywhere.
/// Variants are read only from pnpr's document: npm's documents describe
/// the version as npm clients read it.
pub(super) fn with_platform_variants(
    resolution: LockfileResolution,
    picked: &PackageVersion,
    source: &RegistryResolutionSource<'_>,
    revision: Option<TarballRevision>,
) -> Result<LockfileResolution, ResolveError> {
    let variants = picked.dist.pnpr
        .as_ref()
        .filter(|pnpr| pnpr.install_v2.is_some())
        .and_then(|pnpr| pnpr.variants.as_ref());
    let Some(variants) = variants else { return Ok(resolution) };
    let revision = revision.map(TarballRevision::get);
    let mut selectable = Vec::with_capacity(variants.len());
    for variant in variants {
        if let Some(platforms) = known_platforms(variant) {
            selectable.push((
                variant_resolution(variant, &platforms, picked, source, revision)?,
                platforms,
            ));
        }
    }
    if selectable.is_empty() {
        return Ok(resolution);
    }
    selectable.sort_by_cached_key(|(_, platforms)| variant_tag(platforms).unwrap_or_default());
    let variants = selectable
        .into_iter()
        .map(|(variant, _)| variant)
        .collect();
    Ok(LockfileResolution::Variations(VariationsResolution {
        variants,
        fallback: Some(Box::new(resolution)),
        revision,
    }))
}

/// The variant's platforms, or `None` when one has a field this client does
/// not know: such a variant is not selectable, so it is left out.
fn known_platforms(variant: &PnprVariant) -> Option<Vec<Platform>> {
    variant.platforms
        .iter()
        .map(|platform| serde_json::from_value(platform.clone()).ok())
        .collect()
}

fn variant_resolution(
    variant: &PnprVariant,
    platforms: &[Platform],
    picked: &PackageVersion,
    source: &RegistryResolutionSource<'_>,
    revision: Option<u64>,
) -> Result<PlatformAssetResolution, ResolveError> {
    let version = picked.version.to_string();
    let invalid = || {
        Box::new(MissingTarballUrlError {
            package: format!("{}@{version} variant {}", picked.name, variant.tag),
            registry: redact_and_sanitize(source.registry),
        }) as ResolveError
    };
    let integrity = variant.integrity.parse().map_err(|_| invalid())?;
    // The lockfile names the variant by the tag its targets give, so the
    // route is built from that, not from the document's spelling of it.
    let tag = pnpm_platform_variants::variant_tag(platforms).map_err(|_| invalid())?;
    let tarball = pnpm_lockfile::variant_registry_tarball_url(
        &picked.name,
        &version,
        &tag,
        revision,
        source.registry,
    )
    .ok_or_else(invalid)?;
    Ok(PlatformAssetResolution {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball,
            integrity: Some(integrity),
            revision: None,
            git_hosted: None,
            path: None,
        }),
        targets: platforms
            .iter()
            .cloned()
            .map(PlatformAssetTarget::from_platform)
            .collect(),
    })
}

#[cfg(test)]
mod tests;
