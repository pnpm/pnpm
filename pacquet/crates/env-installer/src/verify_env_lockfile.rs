//! Offline structural gate for the env lockfile, mirroring pnpm's
//! [`verifyEnvLockfile`](https://github.com/pnpm/pnpm/blob/main/installing/env-installer/src/verifyEnvLockfile.ts)
//! and the always-on alias/shape checks `verifyLockfileResolutions` runs over
//! the main lockfile.

use crate::ConfigDepError;
use pacquet_lockfile::{EnvLockfile, PackageKey};
use pacquet_resolving_parse_wanted_dependency::is_valid_old_npm_package_name;
use std::path::Path;

/// Persist an env lockfile only after verifying it, so no code path can write
/// one carrying an invalid config-dependency name or version.
pub fn write_verified_env_lockfile(
    env_lockfile: &EnvLockfile,
    root_dir: &Path,
) -> Result<(), ConfigDepError> {
    verify_env_lockfile(env_lockfile)?;
    env_lockfile.write(root_dir).map_err(ConfigDepError::WriteLockfile)
}

/// Reject config-dependency and optional-subdep names/versions before they
/// build store paths (`<name>/<version>/<hash>`): names must be valid npm
/// package names, versions exact semver — otherwise a traversal-shaped value
/// would escape the install roots.
pub fn verify_env_lockfile(env_lockfile: &EnvLockfile) -> Result<(), ConfigDepError> {
    let Some(importer) = env_lockfile.importers.get(EnvLockfile::ROOT_IMPORTER_KEY) else {
        return Ok(());
    };
    for (name, spec) in &importer.config_dependencies {
        assert_valid_name(name, "The configDependencies in pnpm-lock.yaml")?;
        assert_valid_version(name, &spec.version)?;

        let Ok(key) = format!("{name}@{}", spec.version).parse::<PackageKey>() else {
            continue;
        };
        let Some(optionals) = env_lockfile
            .snapshots
            .get(&key)
            .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
        else {
            continue;
        };
        let description =
            format!("The optionalDependencies of config dependency \"{name}\" in pnpm-lock.yaml");
        for (subdep_name, dep_ref) in optionals {
            let subdep_name = subdep_name.to_string();
            assert_valid_name(&subdep_name, &description)?;
            let version = dep_ref.ver_peer().map(ToString::to_string).unwrap_or_default();
            assert_valid_version(&subdep_name, &version)?;
        }
    }
    Ok(())
}

fn assert_valid_name(name: &str, description: &str) -> Result<(), ConfigDepError> {
    if is_valid_old_npm_package_name(name) {
        Ok(())
    } else {
        Err(ConfigDepError::InvalidDependencyName {
            description: description.to_string(),
            name: name.to_string(),
        })
    }
}

fn assert_valid_version(name: &str, version: &str) -> Result<(), ConfigDepError> {
    if version.parse::<node_semver::Version>().is_err() {
        Err(ConfigDepError::InvalidConfigDepVersion {
            name: name.to_string(),
            version: version.to_string(),
        })
    } else {
        Ok(())
    }
}
