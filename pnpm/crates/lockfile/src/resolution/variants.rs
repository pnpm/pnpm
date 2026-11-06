use super::{
    LockfileFormError, LockfileFormOptions, LockfileResolution, PlatformAssetResolution,
    PlatformSelector, RegistryResolution, VariationsResolution, select_platform_variant,
};
use pnpm_platform_variants::{AmbiguousSelection, Host, Libc, Platform, Purpose};
use serde::{Deserialize, Serialize};

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
/// `cpu`. The target `{ any: true }` has no other field: its variant is the
/// archive of every host that no other variant matches.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields are the flat keys of a lockfile target, which deny_unknown_fields \
                  keeps from being grouped by serde flattening."
    )
)]
pub struct PlatformAssetTarget {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub any: bool,
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
    /// The target `any`.
    #[must_use]
    pub fn any() -> Self {
        PlatformAssetTarget { any: true, ..PlatformAssetTarget::default() }
    }

    /// The target in the platform-variants vocabulary, or `None` for `any`
    /// and for a C library the vocabulary does not know, neither of which
    /// matches a host by its platform.
    #[must_use]
    pub fn platform(&self) -> Option<Platform> {
        if self.any {
            return None;
        }
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
            any: false,
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
    /// The variant `resolution` installs on every host that no other
    /// variant matches.
    #[must_use]
    pub fn any(resolution: LockfileResolution) -> Self {
        PlatformAssetResolution { resolution, targets: vec![PlatformAssetTarget::any()] }
    }

    /// Whether this is the variant that targets `any`.
    #[must_use]
    pub fn is_any(&self) -> bool {
        self.targets.iter().any(|target| target.any)
    }

    fn platforms(&self) -> Vec<Platform> {
        self.targets
            .iter()
            .filter_map(PlatformAssetTarget::platform)
            .collect()
    }

    /// A package variant's name in its registry route: its targets' platform
    /// tags, sorted and joined with `+`. `None` for the variant that targets
    /// `any`, which `dist.tarball` names and has no such route.
    #[must_use]
    pub fn variant_tag(&self) -> Option<String> {
        if self.is_any() {
            return None;
        }
        pnpm_platform_variants::variant_tag(&self.platforms()).ok()
    }
}

impl PlatformSelector {
    pub(crate) fn host(&self) -> Host {
        Host {
            os: self.os.clone(),
            cpu: self.cpu.clone(),
            libc: (self.libc.as_deref() == Some("musl")).then_some(Libc::Musl),
            floors: self.floors,
        }
    }
}

impl VariationsResolution {
    /// A runtime's variants: a host no variant matches fails.
    #[must_use]
    pub fn new(variants: Vec<PlatformAssetResolution>) -> Self {
        VariationsResolution { variants, revision: None }
    }

    /// Whether these are a runtime's archives (Node.js, Bun, Deno), which
    /// are `binary` resolutions, rather than a registry package's.
    #[must_use]
    pub fn is_runtime(&self) -> bool {
        self.variants
            .iter()
            .any(|variant| matches!(variant.resolution, LockfileResolution::Binary(_)))
    }

    /// Every archive some host can install.
    pub fn resolutions(&self) -> impl Iterator<Item = &LockfileResolution> {
        self.variants.iter().map(|variant| &variant.resolution)
    }

    /// [`Self::resolutions`], mutably.
    pub fn resolutions_mut(&mut self) -> impl Iterator<Item = &mut LockfileResolution> {
        self.variants.iter_mut().map(|variant| &mut variant.resolution)
    }

    /// The variant that targets `any`, if there is one.
    #[must_use]
    pub fn any_variant(&self) -> Option<&PlatformAssetResolution> {
        self.variants.iter().find(|variant| variant.is_any())
    }

    /// The variant that fits `platform` best, comparing each variant by its
    /// best matching target, or the variant that targets `any` when none
    /// matches. `None` when neither exists.
    ///
    /// A runtime's archives keep the first match: a runtime lists an
    /// emulated platform as a target of the build that runs there, so two
    /// of its archives can fit one host equally well.
    pub fn select(
        &self,
        platform: &PlatformSelector,
    ) -> Result<Option<&PlatformAssetResolution>, AmbiguousSelection> {
        if self.is_runtime() {
            return Ok(
                select_platform_variant(&self.variants, platform).or_else(|| self.any_variant())
            );
        }
        let platforms: Vec<Vec<Platform>> = self.variants
            .iter()
            .map(PlatformAssetResolution::platforms)
            .collect();
        let selected =
            pnpm_platform_variants::select(platforms.iter().map(Vec::as_slice), &platform.host())?;
        Ok(selected
            .map(|index| &self.variants[index])
            .or_else(|| self.any_variant()))
    }

    /// The archives a store keeps for `platform`, which may not be the
    /// host: every platform variant one of whose targets matches it, its
    /// floors and ABI met unless `platform` states otherwise
    /// ([`Purpose::Fetch`]), and the variant that targets `any` when no
    /// platform variant can be linked there.
    #[must_use]
    pub fn fetched_for(&self, platform: &PlatformSelector) -> Vec<&PlatformAssetResolution> {
        let host = platform.host();
        let mut fetched: Vec<&PlatformAssetResolution> = self.variants
            .iter()
            .filter(|variant| {
                !variant.is_any()
                    && variant
                        .platforms()
                        .iter()
                        .any(|target| target.matches_for(&host, Purpose::Fetch))
            })
            .collect();
        let links_a_platform_variant = match self.select(platform) {
            Ok(selected) => selected.is_some_and(|variant| !variant.is_any()),
            // Variants tied for the platform are linkable ones.
            Err(AmbiguousSelection { .. }) => true,
        };
        if !links_a_platform_variant {
            fetched.extend(self.any_variant());
        }
        fetched
    }

    /// `variant` in the integrity-only registry form when its tarball sits on
    /// its registry route, else as it is.
    fn registry_variant(
        &self,
        variant: &PlatformAssetResolution,
        (name, version): (&str, &str),
        opts: LockfileFormOptions<'_>,
    ) -> PlatformAssetResolution {
        let mut variant = variant.clone();
        if let LockfileResolution::Tarball(tarball) = &variant.resolution
            && let Some(integrity) = &tarball.integrity
            && let Some(tag) = variant.variant_tag()
            && !opts.include_tarball_url
            && variant_registry_tarball_url(name, version, &tag, self.revision, opts.registry)
                .as_deref()
                == Some(tarball.tarball.as_str())
        {
            variant.resolution = LockfileResolution::Registry(RegistryResolution {
                integrity: integrity.clone(),
                revision: None,
            });
        }
        variant
    }

    /// Write each package variant whose tarball sits on its registry route,
    /// and the variant that targets `any` as its version's archive is
    /// written, in the integrity-only registry form.
    pub(super) fn to_lockfile_form(
        &self,
        name: &str,
        version: &str,
        opts: LockfileFormOptions<'_>,
    ) -> Result<VariationsResolution, LockfileFormError> {
        let variants = self.variants
            .iter()
            .map(|variant| {
                if !variant.is_any() {
                    return Ok(self.registry_variant(variant, (name, version), opts));
                }
                let resolution = variant.resolution.to_lockfile_form(name, version, opts)?;
                Ok(PlatformAssetResolution { resolution, targets: variant.targets.clone() })
            })
            .collect::<Result<_, LockfileFormError>>()?;
        Ok(VariationsResolution { variants, revision: self.revision })
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
