use super::{
    super::{Config, InstallError, LayoutVersion, Path},
    normalized_pattern,
};

/// The `validateModules` half pacquet enforces: when the mutation is
/// not a plain install (upstream `installsOnly === false`), a modules
/// directory whose recorded layout disagrees with the current config fails
/// instead of being recreated. Check order matches upstream
/// `validateModules`, then its `checkCompatibility`. A `nodeLinker` drift
/// still takes the recreate path: upstream does not check it.
pub(in super::super) fn check_modules_settings_diff(
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

/// Upstream compares with `path.relative`: lexically, and ignoring case on
/// Windows.
fn same_path(left: &Path, right: &Path) -> bool {
    let left = pnpm_fs::lexical_normalize(left);
    let right = pnpm_fs::lexical_normalize(right);
    if cfg!(windows) {
        left.as_os_str().eq_ignore_ascii_case(right.as_os_str())
    } else {
        left == right
    }
}
