use super::{
    LockfileFormError, LockfileFormOptions, LockfileResolution, PlatformAssetResolution,
    PlatformSelector, RegistryResolution, VariationsResolution, select_platform_variant,
};
use pnpm_platform_variants::{AmbiguousSelection, Host, Libc, Platform};
use serde::{Deserialize, Serialize};

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

/// One `(os, cpu, libc?)` triple a [`PlatformAssetResolution`] covers.
///
/// `libc` is only written for musl-built variants; glibc is the
/// implicit default on Linux and the field is omitted everywhere
/// else. `Option<String>` (rather than `Option<Libc>` enum) keeps
/// future libc values future-compatible without a churning serde
/// migration if a new one lands.
///
/// A package platform variant's target may also carry the floor and ABI
/// fields of the platform-variants vocabulary (see
/// [`pnpm_platform_variants::Platform`]); a `wasm32` target has no `os` or
/// `cpu`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlatformAssetTarget {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cpu: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libc_version: Option<pnpm_platform_variants::Floor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<pnpm_platform_variants::Floor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub napi: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_abi: Option<pnpm_platform_variants::NodeAbi>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wasm32: bool,
}

impl PlatformAssetTarget {
    /// The target in the platform-variants vocabulary, or `None` for a C
    /// library the vocabulary does not know, which never matches.
    #[must_use]
    pub fn platform(&self) -> Option<Platform> {
        let libc = match self.libc.as_deref() {
            None => None,
            Some("glibc") => Some(Libc::Glibc),
            Some("musl") => Some(Libc::Musl),
            Some(_) => return None,
        };
        let present = |value: &str| (!value.is_empty()).then(|| value.to_string());
        Some(Platform {
            os: present(&self.os),
            cpu: present(&self.cpu),
            libc,
            libc_version: self.libc_version,
            os_version: self.os_version,
            napi: self.napi,
            node_abi: self.node_abi,
            wasm32: self.wasm32,
        })
    }

    /// The lockfile's target for a platform: glibc is written as no `libc`.
    #[must_use]
    pub fn from_platform(platform: Platform) -> Self {
        let platform = platform.without_explicit_glibc();
        PlatformAssetTarget {
            os: platform.os.unwrap_or_default(),
            cpu: platform.cpu.unwrap_or_default(),
            libc: platform.libc.map(|libc| {
                match libc {
                    Libc::Glibc => "glibc",
                    Libc::Musl => "musl",
                }
                .to_string()
            }),
            libc_version: platform.libc_version,
            os_version: platform.os_version,
            napi: platform.napi,
            node_abi: platform.node_abi,
            wasm32: platform.wasm32,
        }
    }
}

impl PlatformAssetResolution {
    fn platforms(&self) -> Vec<Platform> {
        self.targets
            .iter()
            .filter_map(PlatformAssetTarget::platform)
            .collect()
    }

    /// A package variant's name in its registry route: its targets' platform
    /// tags, sorted and joined with `+`.
    #[must_use]
    pub fn variant_tag(&self) -> Option<String> {
        pnpm_platform_variants::variant_tag(&self.platforms()).ok()
    }
}

impl PlatformSelector {
    fn host(&self) -> Host {
        Host {
            os: self.os.clone(),
            cpu: self.cpu.clone(),
            libc: (self.libc.as_deref() == Some("musl")).then_some(Libc::Musl),
            floors: self.floors,
        }
    }
}

impl VariationsResolution {
    /// Variants with no fallback, as a runtime resolves: a host no variant
    /// matches fails.
    #[must_use]
    pub fn without_fallback(variants: Vec<PlatformAssetResolution>) -> Self {
        VariationsResolution { variants, fallback: None, revision: None }
    }

    /// The variant that fits `platform` best, comparing each variant by its
    /// best matching target, or the fallback when none matches. `None` when
    /// neither exists.
    ///
    /// A runtime's archives keep the first match: a runtime lists an
    /// emulated platform as a target of the build that runs there, so two
    /// of its archives can fit one host equally well.
    pub fn select(
        &self,
        platform: &PlatformSelector,
    ) -> Result<Option<SelectedVariant<'_>>, AmbiguousSelection> {
        if self.variants
            .iter()
            .any(|variant| matches!(variant.resolution, LockfileResolution::Binary(_)))
        {
            return Ok(select_platform_variant(&self.variants, platform)
                .map(SelectedVariant::Variant)
                .or_else(|| self.fallback.as_deref().map(SelectedVariant::Fallback)));
        }
        let platforms: Vec<Vec<Platform>> = self.variants
            .iter()
            .map(PlatformAssetResolution::platforms)
            .collect();
        let selected =
            pnpm_platform_variants::select(platforms.iter().map(Vec::as_slice), &platform.host())?;
        Ok(selected
            .map(|index| SelectedVariant::Variant(&self.variants[index]))
            .or_else(|| self.fallback.as_deref().map(SelectedVariant::Fallback)))
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
                if let LockfileResolution::Tarball(tarball) = &variant.resolution
                    && let Some(integrity) = &tarball.integrity
                    && let Some(tag) = variant.variant_tag()
                    && !opts.include_tarball_url
                    && variant_registry_tarball_url(
                        name,
                        version,
                        &tag,
                        self.revision,
                        opts.registry,
                    )
                    .as_deref()
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
        Ok(VariationsResolution { variants, fallback, revision: self.revision })
    }
}

/// Build a package variant's URL,
/// `<registry>/<name>@<version>[+r<revision>]/variants/<tag>`.
#[must_use]
pub fn variant_registry_tarball_url(
    name: &str,
    version: &str,
    tag: &str,
    revision: Option<u64>,
    registry: &str,
) -> Option<String> {
    if !pnpm_platform_variants::is_valid_variant_tag(tag) {
        return None;
    }
    let base = format!("{}/", registry.trim_end_matches('/'));
    let release = match revision {
        Some(revision) => format!("{version}+r{revision}"),
        None => version.to_string(),
    };
    url::Url::parse(&base)
        .ok()?
        .join(&format!("{name}@{release}/variants/{tag}"))
        .ok()
        .map(Into::into)
}
