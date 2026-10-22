use pnpm_config::{Config, SaveWorkspaceProtocol};
use pnpm_registry::RangeSpecStyle;
use pnpm_resolving_npm_resolver::{
    DeclaredSpecifiers, calc_specifier_for_workspace_dep, can_drop_workspace_protocol,
    parse_bare_specifier, pick_matching_local_version_or_null, pick_registry_for_package,
};
use pnpm_resolving_resolver_base::WorkspacePackages;
use pnpm_workspace_range_resolver::resolve_workspace_range;
use pnpm_workspace_spec::WorkspaceSpec;

/// Index the workspace projects by name and version, or `None` when
/// there is no workspace or it cannot be enumerated.
///
/// A failure to walk the workspace is not this function's problem to
/// report — the install that follows the manifest write surfaces it with
/// far more context — so it degrades to "no workspace packages", which
/// only costs the pinned form its version.
pub(super) fn workspace_packages_for_add(config: &Config) -> Option<WorkspacePackages> {
    let workspace_dir = config.workspace_dir.as_ref()?;
    let manifest = pnpm_workspace::read_workspace_manifest(workspace_dir).ok()??;
    let projects = pnpm_workspace::find_workspace_projects(
        workspace_dir,
        &pnpm_workspace::FindWorkspaceProjectsOpts {
            patterns: Some(pnpm_workspace::workspace_package_patterns(&manifest)),
            ignored_directories: config.managed_directories(),
        },
    )
    .ok()?;
    crate::install::build_workspace_packages_map(Some(&projects))
}
/// The `workspace:` specifier to save for `package_name`, or `None`
/// when this add isn't a workspace dependency.
///
/// A relative `workspace:./pkg` is left alone: it names a directory, not
/// a range, so there is no operator to roll.
pub(super) fn workspace_save_specifier(
    package_name: &str,
    explicit_spec: Option<&str>,
    prev_specifier: Option<&str>,
    config: &Config,
    range_spec_style: RangeSpecStyle,
    workspace_packages: Option<&WorkspacePackages>,
) -> Option<String> {
    let (target_name, resolved_version) = match explicit_spec.and_then(WorkspaceSpec::parse) {
        Some(spec) => explicit_workspace_target(spec, package_name, workspace_packages)?,
        None => implicit_workspace_target(package_name, explicit_spec, config, workspace_packages)?,
    };
    let workspace_specifier = calc_specifier_for_workspace_dep(
        DeclaredSpecifiers { prev: prev_specifier, bare: explicit_spec },
        Some(package_name),
        &target_name,
        resolved_version.as_deref(),
        config.save_workspace_protocol,
        range_spec_style,
        false,
    );
    if config.save_workspace_protocol == SaveWorkspaceProtocol::Off
        && !explicit_spec.is_some_and(|specifier| specifier.starts_with("workspace:"))
        && resolved_version.as_deref().is_none_or(can_drop_workspace_protocol)
    {
        return workspace_specifier.strip_prefix("workspace:").map(str::to_string);
    }
    Some(workspace_specifier)
}
/// The workspace package a `workspace:` specifier names. A path form
/// (`workspace:./pkg`) names no package here.
pub(super) fn explicit_workspace_target(
    spec: WorkspaceSpec,
    package_name: &str,
    workspace_packages: Option<&WorkspacePackages>,
) -> Option<(String, Option<String>)> {
    if spec.version.starts_with('.') {
        return None;
    }
    let target_name = spec.alias.unwrap_or_else(|| package_name.to_string());
    let resolved_version = workspace_packages
        .and_then(|packages| packages.get(&target_name))
        .and_then(|versions| {
            let available: Vec<String> = versions.keys().cloned().collect();
            // Not `spec.version`: the pinned form records the local
            // package's own version, which wins over the range the
            // user typed.
            resolve_workspace_range("*", &available)
        });
    Some((target_name, resolved_version))
}
/// The workspace package a plain registry request resolves to locally, when
/// `linkWorkspacePackages` lets it.
pub(super) fn implicit_workspace_target(
    package_name: &str,
    explicit_spec: Option<&str>,
    config: &Config,
    workspace_packages: Option<&WorkspacePackages>,
) -> Option<(String, Option<String>)> {
    if !config.link_workspace_packages.enabled_at_depth(0) {
        return None;
    }
    if explicit_spec.is_some_and(|specifier| specifier.starts_with("npm:")) {
        return None;
    }
    let registries: std::collections::HashMap<String, String> = config
        .resolved_registries()
        .into_iter()
        .collect();
    let registry = pick_registry_for_package(&registries, package_name, explicit_spec);
    let parsed = parse_bare_specifier(
        explicit_spec.unwrap_or("latest"),
        Some(package_name),
        "latest",
        &registry,
    )?;
    if parsed.name != package_name || parsed.normalized_bare_specifier.is_some() {
        return None;
    }
    let versions = workspace_packages?.get(package_name)?;
    let resolved_version = pick_matching_local_version_or_null(versions, &parsed)?;
    Some((package_name.to_string(), Some(resolved_version)))
}
