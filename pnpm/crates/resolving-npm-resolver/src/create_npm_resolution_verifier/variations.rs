//! A `variations` resolution is verified archive by archive: each
//! variant, then the fallback that a host no variant matches installs.

use super::NpmResolutionVerifier;
use pnpm_lockfile::LockfileResolution;
use pnpm_resolving_resolver_base::{ResolutionVerification, VerifyCtx};

impl NpmResolutionVerifier {
    pub(super) async fn verify_variations(
        &self,
        variations: &pnpm_lockfile::VariationsResolution,
        ctx: VerifyCtx<'_>,
    ) -> ResolutionVerification {
        for variant in &variations.variants {
            let on_route = self.variant_on_its_route(variations, variant, &ctx);
            let resolution = on_route.as_ref().unwrap_or(&variant.resolution);
            let verification = Box::pin(self.verify_impl(resolution, ctx)).await;
            if !matches!(verification, ResolutionVerification::Ok) {
                return verification;
            }
        }
        match variations.fallback.as_deref() {
            Some(fallback) => Box::pin(self.verify_impl(fallback, ctx)).await,
            None => ResolutionVerification::Ok,
        }
    }

    /// A variant whose tarball URL is its own registry route,
    /// `<name>@<version>[+r<revision>]/variants/<tag>`, in the
    /// integrity-only registry form, which a lockfile writes when it leaves
    /// out tarball URLs. The version's `dist.tarball` names the primary, so
    /// binding that URL to it would refuse every variant. A URL off the
    /// route stays bound to `dist.tarball`, and so is refused.
    fn variant_on_its_route(
        &self,
        variations: &pnpm_lockfile::VariationsResolution,
        variant: &pnpm_lockfile::PlatformAssetResolution,
        ctx: &VerifyCtx<'_>,
    ) -> Option<LockfileResolution> {
        let LockfileResolution::Tarball(tarball) = &variant.resolution else { return None };
        let integrity = tarball.integrity.clone()?;
        let registry = match self.routing.named_registry_url(ctx.registry_name) {
            Ok(Some(registry)) => registry,
            Ok(None) => self.routing.pick_registry(ctx.name, Some(&tarball.tarball)),
            Err(_) => return None,
        };
        let route = pnpm_lockfile::variant_registry_tarball_url(
            &ctx.name.to_string(),
            ctx.version,
            &variant.variant_tag()?,
            variations.revision,
            &registry,
        )?;
        (route == tarball.tarball).then(|| {
            LockfileResolution::Registry(pnpm_lockfile::RegistryResolution {
                integrity,
                revision: None,
            })
        })
    }
}
