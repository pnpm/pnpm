use crate::{
    ConfigDepError, ConfigDependencyVerification, parse_integrity::parse_integrity,
    verify_env_lockfile::verify_env_lockfile,
};
use pnpm_lockfile::{EnvLockfile, LockfileResolution, PackageKey};
use pnpm_resolving_resolver_base::{ResolutionVerification, VerifyCtx};
use pnpm_workspace_state::ConfigDependency;
use ssri::Integrity;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) async fn verify_config_dep_resolutions(
    env: &EnvLockfile,
    declarations: &BTreeMap<String, ConfigDependency>,
    verification: &ConfigDependencyVerification<'_>,
) -> Result<(), ConfigDepError> {
    verify_env_lockfile(env)?;
    let pins = configured_pins(declarations)?;
    assert_configured_pins(env, &pins)?;
    for key in config_dependency_keys(env)? {
        let metadata = env.packages
            .get(&key)
            .ok_or_else(|| ConfigDepError::BadConfigDep {
                message: format!(r#"Missing configuration dependency "{key}""#),
            })?;
        if pins.contains_key(&key) {
            continue;
        }
        let version = key.suffix.version().to_string();
        for verifier in &verification.resolution_verifiers {
            let outcome = verifier.verify(
                &metadata.resolution,
                VerifyCtx { name: &key.name, version: &version, registry_name: None },
            )
            .await;
            check_verification(&key, outcome)?;
        }
    }
    Ok(())
}

fn configured_pins(
    declarations: &BTreeMap<String, ConfigDependency>,
) -> Result<HashMap<PackageKey, Integrity>, ConfigDepError> {
    let mut pins = HashMap::new();
    for (name, declaration) in declarations {
        let spec = match declaration {
            ConfigDependency::Detailed(detail) => &detail.integrity,
            ConfigDependency::VersionWithIntegrity(spec) => spec,
        };
        if !spec.contains('+') {
            continue;
        }
        let (version, integrity) = parse_integrity(name, spec)?;
        let key = config_dependency_key(name, &version)?;
        pins.insert(key, integrity);
    }
    Ok(pins)
}

fn assert_configured_integrity(
    key: &PackageKey,
    resolution: &LockfileResolution,
    pin: &Integrity,
) -> Result<(), ConfigDepError> {
    let integrity = match resolution {
        LockfileResolution::Registry(resolution) => Some(&resolution.integrity),
        LockfileResolution::Tarball(resolution) => resolution.integrity.as_ref(),
        _ => None,
    };
    if integrity == Some(pin) {
        return Ok(());
    }
    Err(ConfigDepError::BadConfigDep {
        message: format!(
            r#"Configuration dependency "{key}" does not match its configured integrity"#,
        ),
    })
}

fn check_verification(
    key: &PackageKey,
    outcome: ResolutionVerification,
) -> Result<(), ConfigDepError> {
    match outcome {
        ResolutionVerification::Ok => Ok(()),
        ResolutionVerification::Err { reason, .. } => Err(ConfigDepError::BadConfigDep {
            message: format!(r#"Configuration dependency "{key}" {reason}"#),
        }),
        ResolutionVerification::FetchFailed { message } => {
            Err(ConfigDepError::BadConfigDep { message })
        }
    }
}

fn assert_configured_pins(
    env: &EnvLockfile,
    pins: &HashMap<PackageKey, Integrity>,
) -> Result<(), ConfigDepError> {
    for (key, pin) in pins {
        let name = key.name.to_string();
        let version = key.suffix.version().to_string();
        let recorded_version = env.importers
            .get(EnvLockfile::ROOT_IMPORTER_KEY)
            .and_then(|importer| importer.config_dependencies.get(&name))
            .map(|dependency| dependency.version.as_str());
        let metadata = env.packages.get(key);
        if recorded_version != Some(version.as_str()) || metadata.is_none() {
            return Err(ConfigDepError::BadConfigDep {
                message: format!(
                    r#"Configuration dependency "{key}" does not match its configured integrity"#,
                ),
            });
        }
        assert_configured_integrity(
            key,
            &metadata.expect("checked config package").resolution,
            pin,
        )?;
    }
    Ok(())
}

fn config_dependency_keys(env: &EnvLockfile) -> Result<HashSet<PackageKey>, ConfigDepError> {
    let mut keys = HashSet::new();
    let Some(importer) = env.importers.get(EnvLockfile::ROOT_IMPORTER_KEY) else {
        return Ok(keys);
    };
    for (name, dependency) in &importer.config_dependencies {
        let key = config_dependency_key(name, &dependency.version)?;
        if let Some(optionals) = env.snapshots
            .get(&key)
            .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
        {
            for (name, reference) in optionals {
                let version = reference
                    .ver_peer()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                keys.insert(config_dependency_key(&name.to_string(), &version)?);
            }
        }
        keys.insert(key);
    }
    Ok(keys)
}

fn config_dependency_key(name: &str, version: &str) -> Result<PackageKey, ConfigDepError> {
    format!("{name}@{version}")
        .parse()
        .map_err(|_| ConfigDepError::BadConfigDep {
            message: format!("Config dependency {name}@{version} has an unparsable lockfile key"),
        })
}
