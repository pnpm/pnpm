use super::{
    super::{Config, InstallError, LayoutVersion, NodeLinker, Path},
    map_node_linker, normalized_pattern,
};
use pnpm_modules_yaml::NodeLinker as ModulesNodeLinker;

/// Refuses a mutation other than a plain install on a drifted modules
/// directory that only a plain install may recreate: the drift
/// [`check_modules_settings_diff`] reports, a `nodeLinker` change, and a
/// `.modules.yaml` that exists but does not parse. Purging for any of them
/// could delete links that a filtered mutation does not recreate, or files
/// pnpm does not own.
pub(in super::super) fn refuse_drifted_modules_dir(
    modules: Option<&pnpm_modules_yaml::ModulesLayout>,
    config: &Config,
    node_linker: NodeLinker,
) -> Result<(), InstallError> {
    let Some(modules) = modules else {
        return Err(InstallError::ModulesBreakingChange {
            modules_dir: config.modules_dir.display().to_string(),
        });
    };
    check_modules_settings_diff(modules, config)?;
    // A manifest from before `nodeLinker` existed omits it; those trees are
    // isolated.
    let recorded_linker = modules.node_linker.unwrap_or(ModulesNodeLinker::Isolated);
    if recorded_linker != map_node_linker(node_linker) {
        return Err(InstallError::NodeLinkerDiff);
    }
    Ok(())
}

/// The `validateModules` half pacquet enforces: when the mutation is
/// not a plain install (upstream `installsOnly === false`), a modules
/// directory whose recorded layout disagrees with the current config fails
/// instead of being recreated. Check order matches upstream
/// `validateModules`, then its `checkCompatibility`. Upstream checks no
/// `nodeLinker` drift.
fn check_modules_settings_diff(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> Result<(), InstallError> {
    if modules.virtual_store_dir_max_length != config.virtual_store_dir_max_length {
        return Err(InstallError::VirtualStoreDirMaxLengthDiff);
    }
    // A `virtualStoreOnly` install (`pnpm fetch`) records empty hoist
    // patterns; the follow-up install completes the linking.
    if modules.virtual_store_only != Some(true) {
        check_hoist_patterns_diff(modules, config)?;
    }
    check_compatibility(modules, config)
}

fn check_hoist_patterns_diff(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> Result<(), InstallError> {
    if normalized_pattern(modules.public_hoist_pattern.as_deref())
        != normalized_pattern(config.public_hoist_pattern.as_deref())
    {
        return Err(InstallError::PublicHoistPatternDiff);
    }
    if normalized_pattern(modules.hoist_pattern.as_deref())
        != normalized_pattern(config.hoist_pattern.as_deref())
    {
        return Err(InstallError::HoistPatternDiff);
    }
    Ok(())
}

/// Upstream `checkCompatibility`: the layout version, the store, and the
/// virtual store the modules directory was linked with.
fn check_compatibility(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> Result<(), InstallError> {
    let modules_dir = config.modules_dir.display().to_string();
    if modules.layout_version != Some(LayoutVersion) {
        return Err(InstallError::ModulesBreakingChange { modules_dir });
    }
    if !recorded_store_matches(&modules.store_dir, config.store_dir.root()) {
        return Err(InstallError::UnexpectedStore {
            expected_store_path: modules.store_dir.clone(),
            actual_store_path: config.store_dir.display().to_string(),
            modules_dir,
        });
    }
    let virtual_store_dir = config.virtual_store_dir();
    if !modules.virtual_store_dir.is_empty()
        && !same_path(Path::new(&modules.virtual_store_dir), virtual_store_dir)
    {
        return Err(InstallError::UnexpectedVirtualStore {
            expected: modules.virtual_store_dir.clone(),
            actual: virtual_store_dir.display().to_string(),
            modules_dir,
        });
    }
    Ok(())
}

/// Upstream also accepts a modules directory linked from the `v3` store
/// next to the current one.
fn recorded_store_matches(recorded: &str, store_dir: &Path) -> bool {
    let recorded = Path::new(recorded);
    !recorded.as_os_str().is_empty()
        && (same_path(recorded, store_dir)
            || store_dir
                .parent()
                .is_some_and(|parent| same_path(recorded, &parent.join("v3"))))
}

/// Upstream compares with `path.relative`: lexically, and on Windows after
/// `toLowerCase`, which folds non-ASCII letters too.
pub(in super::super) fn same_path(left: &Path, right: &Path) -> bool {
    let left = pnpm_fs::lexical_normalize(left);
    let right = pnpm_fs::lexical_normalize(right);
    if cfg!(windows) {
        left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
    } else {
        left == right
    }
}

#[cfg(test)]
mod tests;
