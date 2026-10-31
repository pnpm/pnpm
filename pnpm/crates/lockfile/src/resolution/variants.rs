use super::{
    LockfileFormError, LockfileFormOptions, LockfileResolution, PlatformAssetResolution,
    PlatformSelector, RegistryResolution, VariationsResolution, select_platform_variant,
};

/// The archive a `variations` resolution installs on one platform.
///
/// Where it came from decides how its integrity is read: a variant's is
/// the digest of what its own resolution points at, and the fallback is an
/// ordinary resolution. The resolution's type, not the entry it sits in,
/// decides how it is fetched, so a runtime and a registry package select
/// alike.
#[derive(Debug, Clone, Copy)]
pub enum SelectedVariant<'a> {
    Variant(&'a PlatformAssetResolution),
    Fallback(&'a LockfileResolution),
}

impl<'a> SelectedVariant<'a> {
    #[must_use]
    pub fn resolution(self) -> &'a LockfileResolution {
        match self {
            SelectedVariant::Variant(variant) => &variant.resolution,
            SelectedVariant::Fallback(fallback) => fallback,
        }
    }
}

impl VariationsResolution {
    /// Variants with no fallback, as a runtime resolves: a host no variant
    /// matches fails.
    #[must_use]
    pub fn without_fallback(variants: Vec<PlatformAssetResolution>) -> Self {
        VariationsResolution { variants, fallback: None }
    }

    /// The first variant whose targets include `platform`, or the
    /// fallback when none does. `None` when neither exists.
    #[must_use]
    pub fn select(&self, platform: &PlatformSelector) -> Option<SelectedVariant<'_>> {
        select_platform_variant(&self.variants, platform)
            .map(SelectedVariant::Variant)
            .or_else(|| self.fallback.as_deref().map(SelectedVariant::Fallback))
    }

    /// Write each package variant whose tarball sits on its registry route,
    /// and the fallback, in the integrity-only registry form.
    pub(super) fn to_lockfile_form(
        &self,
        name: &str,
        version: &str,
        opts: LockfileFormOptions<'_>,
    ) -> Result<VariationsResolution, LockfileFormError> {
        let variants = self.variants
            .iter()
            .map(|variant| {
                let mut variant = variant.clone();
                if let (Some(tag), LockfileResolution::Tarball(tarball)) =
                    (&variant.tag, &variant.resolution)
                    && let Some(integrity) = &tarball.integrity
                    && !opts.include_tarball_url
                    && variant_registry_tarball_url(name, version, tag, opts.registry).as_deref()
                        == Some(tarball.tarball.as_str())
                {
                    variant.resolution = LockfileResolution::Registry(RegistryResolution {
                        integrity: integrity.clone(),
                        revision: None,
                    });
                }
                variant
            })
            .collect();
        let fallback = match &self.fallback {
            Some(fallback) => Some(Box::new(fallback.to_lockfile_form(name, version, opts)?)),
            None => None,
        };
        Ok(VariationsResolution { variants, fallback })
    }
}

/// Build a package variant's URL `<registry>/<name>@<version>/variants/<tag>`.
#[must_use]
pub fn variant_registry_tarball_url(
    name: &str,
    version: &str,
    tag: &str,
    registry: &str,
) -> Option<String> {
    let valid_tag = !tag.is_empty()
        && tag
            .bytes()
            .all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            });
    if !valid_tag {
        return None;
    }
    let base = format!("{}/", registry.trim_end_matches('/'));
    url::Url::parse(&base)
        .ok()?
        .join(&format!("{name}@{version}/variants/{tag}"))
        .ok()
        .map(Into::into)
}
