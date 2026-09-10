use super::{
    UpdateError, UpdateView,
    selectors::{ParsedSelector, matcher_one},
};
use pnpm_config::{Config, SaveWorkspaceProtocol};
use pnpm_matcher::create_matcher;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_resolving_npm_resolver::{DeclaredSpecifiers, calc_specifier_for_workspace_dep};
use pnpm_resolving_resolver_base::{WorkspacePackages, WorkspacePackagesByVersion};
use pnpm_workspace_range_resolver::resolve_workspace_range;

/// `--workspace` with nothing to link falls through to the ordinary
/// branches below: a selector that matched no direct dependency still
/// updates that name deeper in the graph, and an empty selector list
/// still updates every direct dependency.
pub(super) fn workspace_targets(
    update: UpdateView<'_>,
    selectors: &[ParsedSelector],
    direct: &[(String, DependencyGroup, String)],
) -> Result<Vec<WorkspaceLinkTarget>, UpdateError> {
    Ok(update
        .workspace_packages
        .map(|packages| workspace_link_targets(selectors, direct, packages, update.config))
        .transpose()?
        .unwrap_or_default())
}
/// One direct dependency `--workspace` re-points at the workspace copy
/// of the same name.
pub(super) struct WorkspaceLinkTarget {
    pub(super) name: String,
    pub(super) group: DependencyGroup,
    /// The specifier the manifest declares today, which decides the range
    /// operator the rewritten `workspace:` specifier keeps.
    declared: String,
    /// The range the selector asked for (`*` for a bare `foo`), which the
    /// workspace version has to satisfy.
    wanted_range: String,
}
/// The direct dependencies `--workspace` re-points, in manifest order.
///
/// With no selectors every direct dependency that a workspace project
/// publishes is linked (minus `updateConfig.ignoreDependencies`); the
/// rest keep their registry specifiers. With selectors, each *matched*
/// direct dependency must be a workspace package — naming one that isn't
/// is the failure the `--workspace` help text advertises.
pub(super) fn workspace_link_targets(
    selectors: &[ParsedSelector],
    direct: &[(String, DependencyGroup, String)],
    workspace_packages: &WorkspacePackages,
    config: &Config,
) -> Result<Vec<WorkspaceLinkTarget>, UpdateError> {
    if selectors.is_empty() {
        return Ok(all_workspace_link_targets(direct, workspace_packages, config));
    }
    let mut targets = Vec::new();
    let patterns = selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
    let matcher = create_matcher(&patterns);
    // Per-selector matchers, compiled once, map a matched dependency back
    // to the selector that claimed it — and so to the version it asked for.
    let claims = selectors
        .iter()
        .map(|selector| (matcher_one(&selector.pattern), selector.version.as_deref()))
        .collect::<Vec<_>>();
    for (name, group, declared) in direct {
        if !matcher.matches(name.as_str()) {
            continue;
        }
        if !workspace_packages.contains_key(name) {
            return Err(UpdateError::WorkspacePackageNotFound(name.clone()));
        }
        let wanted = claims
            .iter()
            .find(|(matcher, _)| matcher.matches(name))
            .and_then(|(_, version)| *version)
            .unwrap_or("*");
        targets.push(WorkspaceLinkTarget {
            name: name.clone(),
            group: *group,
            declared: declared.clone(),
            wanted_range: wanted.strip_prefix("workspace:").unwrap_or(wanted).to_string(),
        });
    }
    Ok(targets)
}
/// Without a selector, `--workspace` relinks every direct dependency the
/// workspace itself provides, minus the ignored ones.
pub(super) fn all_workspace_link_targets(
    direct: &[(String, DependencyGroup, String)],
    workspace_packages: &WorkspacePackages,
    config: &Config,
) -> Vec<WorkspaceLinkTarget> {
    let ignore_patterns = config.update_config.ignore_dependencies.as_deref().unwrap_or_default();
    let ignore_matcher = (!ignore_patterns.is_empty()).then(|| create_matcher(ignore_patterns));
    direct
        .iter()
        .filter(|(name, _, _)| {
            !ignore_matcher.as_ref().is_some_and(|matcher| matcher.matches(name.as_str()))
                && workspace_packages.contains_key(name)
        })
        .map(|(name, group, declared)| WorkspaceLinkTarget {
            name: name.clone(),
            group: *group,
            declared: declared.clone(),
            wanted_range: "*".to_string(),
        })
        .collect()
}
/// The `workspace:` specifier `--workspace` writes for a linked
/// dependency.
///
/// `--workspace` is an explicit request for the protocol, so unlike
/// `pnpm add` this never declines on [`SaveWorkspaceProtocol::Off`] —
/// the setting only chooses the shape.
pub(super) fn workspace_specifier(
    target: &WorkspaceLinkTarget,
    versions: &WorkspacePackagesByVersion,
    protocol: SaveWorkspaceProtocol,
    default_pin: RangeSpecStyle,
) -> String {
    // Nothing satisfies the requested range: keep it, so the install
    // reports it as `NO_MATCHING_VERSION_INSIDE_WORKSPACE` against the
    // range the user asked for.
    let Some(version) = pick_workspace_version(versions, &target.wanted_range) else {
        return format!("workspace:{}", target.wanted_range);
    };
    calc_specifier_for_workspace_dep(
        DeclaredSpecifiers { prev: Some(&target.declared), bare: None },
        None,
        &target.name,
        Some(&version),
        protocol,
        default_pin,
    )
}
/// The workspace version a `workspace:<range>` specifier would resolve
/// to. A range that isn't semver is a dist-tag, which the workspace
/// answers with its highest version.
pub(super) fn pick_workspace_version(
    versions: &WorkspacePackagesByVersion,
    range: &str,
) -> Option<String> {
    let range = if node_semver::Range::parse(range).is_ok() { range } else { "*" };
    resolve_workspace_range(range, &versions.keys().cloned().collect::<Vec<_>>())
}
