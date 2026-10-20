use crate::{ConfigDepError, ConfigDepsInstallMode, install_config_deps::read_dir_names};
use pnpm_lockfile::EnvLockfile;
use pnpm_workspace_state::ConfigDependency;
use std::{collections::BTreeMap, path::Path};

/// Load reconciliation state without opening the store or initializing resolvers.
pub fn read_config_dependency_lockfile(
    config_dependencies: Option<&BTreeMap<String, ConfigDependency>>,
    root_dir: &Path,
    mode: ConfigDepsInstallMode,
) -> Result<Option<EnvLockfile>, ConfigDepError> {
    let has_declarations = config_dependencies.is_some_and(|dependencies| !dependencies.is_empty());
    if mode == ConfigDepsInstallMode::Bootstrap && !has_declarations {
        return Ok(None);
    }
    let env = EnvLockfile::read(root_dir).map_err(ConfigDepError::ReadLockfile)?;
    if has_declarations
        || env
            .as_ref()
            .is_some_and(|env| {
                env.importers
                    .get(EnvLockfile::ROOT_IMPORTER_KEY)
                    .is_some_and(|importer| !importer.config_dependencies.is_empty())
            })
    {
        return Ok(Some(env.unwrap_or_else(EnvLockfile::create)));
    }
    let installed = read_dir_names(&root_dir.join("node_modules/.pnpm-config"))?;
    Ok((!installed.is_empty()).then(|| env.unwrap_or_else(EnvLockfile::create)))
}
