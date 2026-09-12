use super::{AddError, AddResolution, AddResolveInputs, AddView};
use crate::{
    resolution_policy::{PickPolicy, pick_package_context},
    resolve_latest::LatestPicker,
};
use pnpm_config::Config;
use pnpm_lockfile_preferred_versions::get_preferred_versions_from_lockfile_and_manifests;
use pnpm_package_manifest::PackageManifest;
use pnpm_registry::RangeSpecStyle;
use pnpm_resolving_npm_resolver::{
    PickPackageOptions, calc_version_range, infer_range_spec_style, parse_bare_specifier,
    pick_package, pick_registry_for_package,
};

/// The range a brand-new dependency is saved with: its `latest` version
/// under the configured range style.
pub(super) async fn pick_latest_range(
    package_name: &str,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<String, AddError> {
    let config = inputs.add.config;
    let latest = inputs
        .resolution
        .latest_picker
        .get_or_try_init(|| {
            std::future::ready(
                PickPolicy::from_config(config)
                    .map(|policy| {
                        LatestPicker::new(
                            config,
                            inputs.add.http_client,
                            policy,
                            std::sync::Arc::clone(&inputs.resolution.meta_cache),
                            std::sync::Arc::clone(&inputs.resolution.fetch_locker),
                        )
                    })
                    .map_err(AddError::MinimumReleaseAgeExclude),
            )
        })
        .await?
        .resolve(package_name, false)
        .await
        .map_err(|error| AddError::ResolveLatest { name: package_name.to_string(), error })?;
    Ok(calc_version_range(&latest.version, None, None, inputs.add.range_spec_style))
}
/// Resolve an explicit `add <name>@<spec>` registry specifier to the
/// manifest range pnpm would record: the spec resolved to a concrete
/// version (through the *same* resolver path the follow-up install uses, so
/// the pinned version equals the version the install locks — `resolutionMode`
/// and `minimumReleaseAge` included), carrying the operator the existing
/// entry pins, then the spec's, then the configured default. So `pnpm add
/// foo@^7` records `^7.8.4`, not `^7`.
///
/// Returns `Ok(None)` — write the specifier verbatim — for anything that is
/// not a plain registry range/tag/version for `package_name` itself:
/// non-registry protocols (`git:`/`file:`/`workspace:`/URLs, which
/// [`parse_bare_specifier`] rejects), `npm:` aliases (resolving them risks
/// dropping the aliased target), and specifiers that resolve to no version.
pub(super) async fn resolve_explicit_registry_spec(
    package_name: &str,
    spec: &str,
    prev_specifier: Option<&str>,
    add: AddView<'_>,
    manifest: &PackageManifest,
    resolution: &AddResolution<'_>,
) -> Result<Option<String>, AddError> {
    if spec.starts_with("npm:") {
        return Ok(None);
    }
    let registry = package_registry(add.config, package_name);
    let Some(spec_parsed) = parse_explicit_registry_spec(package_name, spec, &registry) else {
        return Ok(None);
    };

    let policy = PickPolicy::from_config(add.config).map_err(AddError::MinimumReleaseAgeExclude)?;
    // Bias the pick toward versions already present in the workspace, so a
    // dedup pick matches what the install locks (e.g. a sibling already on
    // `1.2.0` keeps `pnpm add foo@^1` on `1.2.0`). Seeded from the wanted
    // lockfile + this manifest; sibling manifests aren't reachable here, so
    // an unlocked sibling declaration may still differ — never an
    // inconsistency, since the install resolves the rewritten range.
    let preferred_versions = get_preferred_versions_from_lockfile_and_manifests(
        add.lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()),
        &[manifest],
    );
    let ctx = pick_package_context(
        add.http_client,
        add.config,
        &policy,
        &resolution.meta_cache,
        &resolution.fetch_locker,
    );
    let opts = explicit_registry_pick_options(
        add.config,
        &registry,
        &policy,
        preferred_versions.get(package_name),
    );

    let pick = pick_package(&ctx, &spec_parsed, &opts)
        .await
        .map_err(|error| AddError::ResolveSpec(Box::new(error)))?;
    let Some(picked) = pick.picked_package else {
        return Ok(None);
    };

    Ok(Some(saved_registry_range(
        &picked.version,
        package_name,
        &registry,
        prev_specifier,
        spec,
        add.range_spec_style,
    )))
}
/// Registry-host tarball URLs must remain verbatim even though the npm parser accepts them.
pub(super) fn parse_explicit_registry_spec(
    package_name: &str,
    spec: &str,
    registry: &str,
) -> Option<pnpm_resolving_npm_resolver::RegistryPackageSpec> {
    parse_bare_specifier(spec, Some(package_name), "latest", registry)
        .filter(|parsed| parsed.normalized_bare_specifier.is_none() && parsed.name == package_name)
}
/// The explicit range is authoritative; including the latest tag could exceed its bounds.
pub(super) fn explicit_registry_pick_options<'a>(
    config: &'a Config,
    registry: &'a str,
    policy: &'a PickPolicy,
    preferred_version_selectors: Option<&'a pnpm_resolving_resolver_base::VersionSelectors>,
) -> PickPackageOptions<'a> {
    PickPackageOptions {
        registry,
        preferred_version_selectors,
        published_by: policy.published_by,
        published_by_exclude: policy.published_by_exclude.as_ref(),
        pick_lowest_version: policy.pick_lowest_direct,
        include_latest_tag: false,
        dry_run: false,
        optional: false,
        update_checksums: false,
        trust_policy: Some(config.trust_policy),
        blocked_versions: None,
    }
}
// Only registry specifiers contribute a saved range operator; path versions are incidental.
pub(super) fn saved_registry_range(
    version: &node_semver::Version,
    package_name: &str,
    registry: &str,
    prev_specifier: Option<&str>,
    spec: &str,
    range_spec_style: RangeSpecStyle,
) -> String {
    let prev_pin = prev_specifier
        .filter(|prev| is_registry_style_specifier(prev, package_name, registry))
        .and_then(infer_range_spec_style);
    calc_version_range(version, prev_pin, infer_range_spec_style(spec), range_spec_style)
}
/// The registry `package_name` resolves against under the configured scopes.
pub(super) fn package_registry(config: &Config, package_name: &str) -> String {
    let registries: std::collections::HashMap<String, String> =
        config.resolved_registries().into_iter().collect();
    pick_registry_for_package(&registries, package_name, None)
}
/// Whether `specifier` is a plain registry range/tag/version for
/// `package_name` (not a non-registry protocol, path, or tarball URL), and
/// so carries a meaningful range operator.
pub(super) fn is_registry_style_specifier(
    specifier: &str,
    package_name: &str,
    registry: &str,
) -> bool {
    parse_bare_specifier(specifier, Some(package_name), "latest", registry)
        .is_some_and(|parsed| parsed.normalized_bare_specifier.is_none())
}
