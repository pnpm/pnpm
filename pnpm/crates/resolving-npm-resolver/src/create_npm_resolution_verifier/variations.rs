//! A `variations` resolution is verified archive by archive. The variant
//! that targets `any` is the version's archive, verified as one.

use super::{NpmResolutionVerifier, artifact_binding::same_tarball_url};
use pnpm_lockfile::LockfileResolution;
use pnpm_resolving_resolver_base::{ResolutionVerification, VerifyCtx};

impl NpmResolutionVerifier {
    pub(super) async fn verify_variations(
        &self,
        variations: &pnpm_lockfile::VariationsResolution,
        ctx: VerifyCtx<'_>,
    ) -> ResolutionVerification {
        for variant in &variations.variants {
            let verification = match self.variant_route_registry(variations, variant, &ctx) {
                Some(registry) => self.verify_variant_on_its_route(&registry, &ctx).await,
                None => Box::pin(self.verify_impl(&variant.resolution, ctx)).await,
            };
            if !matches!(verification, ResolutionVerification::Ok) {
                return verification;
            }
        }
        ResolutionVerification::Ok
    }

    /// The registry whose route, `<name>@<version>[+r<revision>]/variants/<tag>`,
    /// the variant's tarball URL is, compared as the `any` archive's URL is
    /// compared with `dist.tarball`. `None` for a variant with no URL or no
    /// integrity, and for a URL off the route, which stays bound to the
    /// version's `dist.tarball` and so is refused.
    fn variant_route_registry(
        &self,
        variations: &pnpm_lockfile::VariationsResolution,
        variant: &pnpm_lockfile::PlatformAssetResolution,
        ctx: &VerifyCtx<'_>,
    ) -> Option<String> {
        let LockfileResolution::Tarball(tarball) = &variant.resolution else { return None };
        tarball.integrity.as_ref()?;
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
        same_tarball_url(&route, &tarball.tarball).then_some(registry)
    }

    /// A variant on its own route is verified as the integrity-only registry
    /// form a lockfile writes when it leaves out tarball URLs. Its URL is not
    /// bound to `dist.tarball`, which names the `any` archive, and the policies
    /// read the metadata of `registry`, the one the variant is fetched from.
    async fn verify_variant_on_its_route(
        &self,
        registry: &str,
        ctx: &VerifyCtx<'_>,
    ) -> ResolutionVerification {
        if node_semver::Version::parse(ctx.version).is_err() {
            return ResolutionVerification::Ok;
        }
        let (age_applies, trust_applies) = self.policies_for(ctx);
        if !age_applies && !trust_applies {
            return ResolutionVerification::Ok;
        }
        self.run_policy_checks(registry, ctx, age_applies, trust_applies).await
    }
}
