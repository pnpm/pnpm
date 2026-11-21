use std::{
    collections::BTreeMap,
    sync::Arc,
};

use jiff::Timestamp;
use pnpm_lockfile::{
    RegistryOptions,
    RegistryServerType,
    registry_server_type,
};
use pnpm_resolving_resolver_base::{
    ResolveError,
    ResolveOptions,
    ResolveResult,
    WantedDependency,
    WorkspacePackages,
    lag_minutes,
};
use pnpm_version_policy::PolicyMatch;

use super::{
    NPM_REGISTRY_RESOLVED_VIA,
    NpmResolver,
    PickedFromRegistry,
    RegistryPick,
    calculated_specifier,
    fast_path_pick,
    trust_check_for_pick,
    validate_revision_selector,
    wanted_spec,
    warn_once_on_trust_downgrade_fallback,
    workspace_packages_active,
    workspace_shadow_pick,
};
use crate::{
    MAX_PNPR_FORMAT_RELEASE_AGE_MINUTES,
    PickPackageError,
    TrustCheckOptions,
    named_registry::pick_registry_for_package,
    pick_package::PackageMetaCache,
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

        let picked = match self.checked_pick(registry.url, spec, opts, optional).await {
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

    /// [`Self::pick_from_registry`] under the trust check for the pick, once
    /// [`check_pnpr_trust_window`] has passed its window. A refused window is
    /// an unpicked outcome, so a matching workspace package still satisfies
    /// the dependency.
    async fn checked_pick(
        &self,
        registry: &str,
        spec: &RegistryPackageSpec,
        opts: &ResolveOptions,
        optional: bool,
    ) -> Result<RegistryPick, ResolveError> {
        let trust_check = trust_check_for_pick(opts, self.cache_policy.ignore_missing_time_field);
        let registry_options = &self.format.registry_options_by_url;
        let package = PnprPackage { registry, name: &spec.name };
        check_pnpr_trust_window(registry_options, package, trust_check.as_ref(), Timestamp::now())?;
        self.pick_from_registry(registry, spec, opts, optional, trust_check).await
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

/// A package and the registry it is picked from.
#[derive(Clone, Copy)]
pub(super) struct PnprPackage<'a> {
    pub registry: &'a str,
    pub name: &'a str,
}

/// Refuse a `trustPolicyIgnoreAfter` window that counts back from
/// `referenceTime` when pnpr's own package document cannot date it,
/// before reading one from the registry. The document dates versions only
/// within 30 days of the package's last publish, and the fixed time's lag
/// behind `clock` moves the window that far further back. A window that
/// counts back from the current time is not refused, and neither is a
/// package `trustPolicyExclude` exempts at every version.
pub(super) fn check_pnpr_trust_window(
    registry_options: &BTreeMap<String, RegistryOptions>,
    package: PnprPackage<'_>,
    trust_check: Option<&TrustCheckOptions<'_>>,
    clock: Timestamp,
) -> Result<(), PickPackageError> {
    let Some(trust_check) = trust_check else {
        return Ok(());
    };
    let (Some(ignore_after), Some(now)) =
        (trust_check.trust_policy_ignore_after_minutes, trust_check.now)
    else {
        return Ok(());
    };
    let exempt = trust_check.trust_policy_exclude.is_some_and(|exclude| {
        matches!(exclude.matches(package.name), PolicyMatch::AnyVersion)
    });
    let is_pnpr =
        registry_server_type(registry_options, package.registry) == Some(RegistryServerType::Pnpr);
    let minutes = ignore_after.saturating_add(lag_minutes(now, clock));
    if is_pnpr && !exempt && minutes >= MAX_PNPR_FORMAT_RELEASE_AGE_MINUTES {
        return Err(PickPackageError::UnsupportedTrustPolicyIgnoreAfter {
            minutes,
            registry: pnpm_network::redact_url_for_display(package.registry),
        });
    }
    Ok(())
}
