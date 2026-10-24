use std::sync::Arc;

use pnpm_lockfile::registry_server_type;
use pnpm_resolving_resolver_base::{
    ResolveError, ResolveOptions, ResolveResult, WantedDependency, WorkspacePackages,
};

use super::{
    NPM_REGISTRY_RESOLVED_VIA, NpmResolver, PickedFromRegistry, RegistryPick, calculated_specifier,
    fast_path_pick, trust_check_for_pick, validate_revision_selector, wanted_spec,
    warn_once_on_trust_downgrade_fallback, workspace_packages_active, workspace_shadow_pick,
};
use crate::{
    named_registry::pick_registry_for_package, pick_package::PackageMetaCache,
    pick_package_from_meta::RegistryPackageSpec,
};

/// The registry a dependency resolves against, and the prefix the lockfile
/// key of a package resolved from it carries (see
/// [`NpmResolver::declared_prefixes`]).
#[derive(Clone, Copy)]
pub(crate) struct ResolvedRegistry<'a> {
    pub url: &'a str,
    pub key_prefix: Option<&'a str>,
}

impl<Cache: PackageMetaCache + 'static> NpmResolver<Cache> {
    pub(super) async fn resolve_registry_dependency(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
        default_tag: &str,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some((registry_url, spec)) =
            self.prepare_registry_spec(wanted_dependency, default_tag)?
        else {
            return Ok(None);
        };
        let registry = ResolvedRegistry {
            url: &registry_url,
            key_prefix: self.declared_prefixes.prefix_for(&registry_url),
        };
        self.resolve_from_registry(wanted_dependency, opts, &spec, registry).await
    }

    async fn resolve_from_registry(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
        spec: &RegistryPackageSpec,
        registry: ResolvedRegistry<'_>,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let optional = wanted_dependency.optional.unwrap_or(false);
        let workspace_packages_active = workspace_packages_active(opts, spec);

        let store_index = self.store_index();
        if let Some(result) = fast_path_pick(
            store_index,
            wanted_dependency,
            opts,
            spec,
            workspace_packages_active,
            registry.key_prefix,
        )
        .await?
        {
            return Ok(Some(result));
        }

        let trust_check = trust_check_for_pick(opts, self.cache_policy.ignore_missing_time_field);
        let picked =
            match self.pick_from_registry(registry.url, spec, opts, optional, trust_check).await {
                Ok(RegistryPick::Picked(picked)) => picked,
                outcome => {
                    return self.unpicked_fallback(
                        outcome,
                        wanted_dependency,
                        registry,
                        workspace_packages_active,
                        spec,
                        opts,
                    )
                    .await;
                }
            };

        self.finish_registry_pick(
            wanted_dependency,
            opts,
            spec,
            registry,
            &picked,
            workspace_packages_active,
        )
    }

    fn prepare_registry_spec(
        &self,
        wanted_dependency: &WantedDependency,
        default_tag: &str,
    ) -> Result<Option<(String, RegistryPackageSpec)>, ResolveError> {
        let registry = pick_registry_for_package(
            &self.registries,
            wanted_dependency.alias.as_deref().unwrap_or_default(),
            wanted_dependency.bare_specifier.as_deref(),
        );
        let Some(spec) = wanted_spec(wanted_dependency, default_tag, &registry) else {
            return Ok(None);
        };
        validate_revision_selector(&spec)?;
        Ok(Some((registry, spec)))
    }

    fn finish_registry_pick(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
        spec: &RegistryPackageSpec,
        registry: ResolvedRegistry<'_>,
        picked: &PickedFromRegistry,
        workspace_packages_active: Option<&Arc<WorkspacePackages>>,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        if let Some(result) =
            workspace_shadow_pick(workspace_packages_active, spec, picked, wanted_dependency, opts)
        {
            return Ok(Some(result));
        }

        warn_once_on_trust_downgrade_fallback(&spec.name, picked);
        self.registry_pick_result(wanted_dependency, opts, spec, registry, picked)
    }

    fn registry_pick_result(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
        spec: &RegistryPackageSpec,
        registry: ResolvedRegistry<'_>,
        picked: &PickedFromRegistry,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        crate::npm_resolver::RegistryResolutionSource {
            resolved_via: NPM_REGISTRY_RESOLVED_VIA,
            registry: registry.url,
            registry_name: registry.key_prefix,
            server_type: registry_server_type(&self.format.registry_options_by_url, registry.url),
        }
        .build_result(
            picked,
            &opts.policy,
            &self.metadata.picked_manifest_cache,
            crate::npm_resolver::ResolvedSpecifier {
                spec,
                alias: wanted_dependency.alias.as_deref(),
                calculated_specifier: calculated_specifier(wanted_dependency, opts, spec, picked),
            },
        )
        .map(Some)
    }
}
