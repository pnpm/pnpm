//! `hoist-workspace-packages` placement: which named workspace projects
//! get a link by name, shared by the isolated hoist pass and the hoisted
//! linker.

use super::HoistedDependencies;
use pnpm_matcher::Matcher;
use pnpm_modules_yaml::HoistKind;
use std::{collections::HashSet, path::PathBuf};

/// `hoist-workspace-packages` placements for the hoisted linker, whose
/// packages all live in the root `node_modules`: a project the patterns
/// select is linked there unless a package of the hoisted tree already
/// holds its name.
#[must_use]
pub fn hoist_workspace_packages_to_root<'a>(
    workspace_packages: &crate::HoistedWorkspacePackages,
    root_aliases: impl IntoIterator<Item = &'a str>,
    private_pattern: &Matcher,
    public_pattern: &Matcher,
) -> WorkspaceHoists {
    let mut hoisted_aliases: HashSet<String> = root_aliases
        .into_iter()
        .map(str::to_lowercase)
        .collect();
    let mut hoists = WorkspaceHoists::default();
    for (name, project_id, dir, hoist_kind) in place_workspace_packages(
        workspace_packages,
        |alias| pattern_hoist_kind(private_pattern, public_pattern, alias),
        &mut hoisted_aliases,
    ) {
        hoists.aliases.push((name.clone(), hoist_kind, dir));
        hoists.hoisted_dependencies
            .entry(project_id)
            .or_default()
            .insert(name, hoist_kind);
    }
    hoists
}

/// What [`hoist_workspace_packages_to_root`] placed.
#[derive(Debug, Default)]
pub struct WorkspaceHoists {
    /// Keyed by project id, as `.modules.yaml` records them.
    pub hoisted_dependencies: HoistedDependencies,
    /// (alias, kind, absolute project dir), the shape
    /// [`super::symlink_hoisted_dependencies`] links.
    pub aliases: Vec<(String, HoistKind, PathBuf)>,
}

/// Which hoist target the configured patterns put `alias` in, if any.
pub(super) fn pattern_hoist_kind(
    private_pattern: &Matcher,
    public_pattern: &Matcher,
    alias: &str,
) -> Option<HoistKind> {
    if public_pattern.matches(alias) {
        Some(HoistKind::Public)
    } else if private_pattern.matches(alias) {
        Some(HoistKind::Private)
    } else {
        None
    }
}

/// The named workspace projects that a hoist pattern selects, whose
/// alias nothing in `hoisted_aliases` holds and no other project
/// conflicts with. Each placed alias is claimed in `hoisted_aliases`.
pub(super) fn place_workspace_packages(
    workspace_packages: &crate::HoistedWorkspacePackages,
    hoist_kind: impl Fn(&str) -> Option<HoistKind>,
    hoisted_aliases: &mut HashSet<String>,
) -> Vec<(String, String, PathBuf, HoistKind)> {
    let candidates = workspace_packages
        .iter()
        .filter_map(|(name, (project_id, dir))| {
            let hoist_kind = hoist_kind(name)?;
            (!hoisted_aliases.contains(&name.to_lowercase())).then(|| {
                (name.clone(), project_id.clone(), dir.clone(), hoist_kind)
            })
        })
        .collect::<Vec<_>>();
    let conflicting_aliases = super::workspace_aliases::conflicting_aliases(
        candidates
            .iter()
            .map(|(name, _, _, kind)| (name.as_str(), *kind)),
    );
    candidates
        .into_iter()
        .filter(|(name, _, _, _)| {
            let normalized_name = name.to_lowercase();
            !conflicting_aliases.contains(&normalized_name)
                && hoisted_aliases.insert(normalized_name)
        })
        .collect()
}
