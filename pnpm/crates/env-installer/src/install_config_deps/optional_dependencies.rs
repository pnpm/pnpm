use super::{
    AtomicU8, BTreeMap, ConfigDepError, ConfigDepsInstallOptions, EnvLockfile,
    InstallabilityOptions, Integrity, LockfileResolution, LogEvent, LogLevel, NormalizedConfigDep,
    NormalizedSubdep, PackageInstallabilityManifest, Path, Reporter, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, SkippedOptionalReason, StartedGate, TarballUrlOptions,
    calc_leaf_global_virtual_store_path, check_package, force_symlink, full_pkg_id,
    join_global_virtual_store_path, materialize, npm_tarball_url, prune_unexpected_siblings,
    symlink_points_to,
};

#[expect(
    clippy::too_many_arguments,
    reason = "this install path threads every resolution input through one call"
)]
pub(super) async fn install_optional_subdeps<Reporter: self::Reporter>(
    opts: &ConfigDepsInstallOptions<'_>,
    logged_methods: &AtomicU8,
    started: &mut StartedGate,
    parent_name: &str,
    parent_version: &str,
    subdeps: &[NormalizedSubdep],
    global_virtual_store_dir: &Path,
    parent_node_modules_dir: &Path,
) -> Result<(), ConfigDepError> {
    let compatible: Vec<&NormalizedSubdep> = subdeps
        .iter()
        .filter(|subdep| is_compatible::<Reporter>(opts, parent_name, parent_version, subdep))
        .collect();

    prune_unexpected_siblings::<Reporter>(
        parent_name,
        &compatible,
        parent_node_modules_dir,
        started,
    )?;

    for subdep in compatible {
        let subdep_full_id = full_pkg_id(&subdep.name, &subdep.version, &subdep.integrity);
        let subdep_rel =
            calc_leaf_global_virtual_store_path(&subdep_full_id, &subdep.name, &subdep.version);
        let subdep_dir = join_global_virtual_store_path(global_virtual_store_dir, &subdep_rel)
            .join("node_modules")
            .join(&subdep.name);
        if !subdep_dir.join("package.json").exists() {
            started.report::<Reporter>();
            materialize::<Reporter>(
                opts,
                logged_methods,
                &subdep.name,
                &subdep.version,
                &subdep.integrity,
                &subdep.tarball,
                &subdep_dir,
            )
            .await?;
        }
        let link_path = parent_node_modules_dir.join(&subdep.name);
        if symlink_points_to(&link_path, &subdep_dir) {
            continue;
        }
        started.report::<Reporter>();
        force_symlink(&subdep_dir, &link_path)?;
    }
    Ok(())
}

/// Whether `subdep` runs on the host. Subdeps with no platform
/// constraints always pass; otherwise [`check_package`] decides, and an
/// incompatible subdep is logged at debug via
/// `pnpm:skipped-optional-dependency`. Uses the non-warning check
/// (rather than the installability check, which would warn loudly on
/// every install because the env lockfile records all platform
/// variants).
fn is_compatible<Reporter: self::Reporter>(
    opts: &ConfigDepsInstallOptions<'_>,
    parent_name: &str,
    parent_version: &str,
    subdep: &NormalizedSubdep,
) -> bool {
    if subdep.os.is_none() && subdep.cpu.is_none() && subdep.libc.is_none() {
        return true;
    }
    let manifest = subdep_installability_manifest(subdep);
    let id = format!("{}@{}", subdep.name, subdep.version);
    let options = InstallabilityOptions {
        current_node_version: opts.current_node_version,
        current_os: opts.current_os,
        current_cpu: opts.current_cpu,
        current_libc: opts.current_libc,
        supported_architectures: opts.supported_architectures,
        ..InstallabilityOptions::default()
    };
    match check_package(&id, &manifest, &options) {
        Ok(None) => true,
        Ok(Some(error)) => {
            Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
                level: LogLevel::Debug,
                details: Some(error.to_string()),
                package: SkippedOptionalPackage::Installed {
                    id,
                    name: subdep.name.clone(),
                    version: subdep.version.clone(),
                },
                parents: None,
                prefix: opts.root_dir.to_string_lossy().into_owned(),
                reason: match error.skip_reason() {
                    pnpm_package_is_installable::SkipReason::UnsupportedEngine => {
                        SkippedOptionalReason::UnsupportedEngine
                    }
                    pnpm_package_is_installable::SkipReason::UnsupportedPlatform => {
                        SkippedOptionalReason::UnsupportedPlatform
                    }
                },
            }));
            let _ = (parent_name, parent_version);
            false
        }
        // An invalid node version on a platform-only subdep is
        // unreachable (engines is `None`); treat the package as
        // installable rather than aborting the whole config-deps pass.
        Err(_) => true,
    }
}

fn subdep_installability_manifest(subdep: &NormalizedSubdep) -> PackageInstallabilityManifest {
    PackageInstallabilityManifest {
        name: subdep.name.clone(),
        engines: None,
        cpu: subdep.cpu.clone(),
        os: subdep.os.clone(),
        libc: subdep.libc.clone(),
    }
}

/// Build the install-set view of `env_lockfile.importers["."]`,
/// surfacing `ENV_LOCKFILE_CORRUPTED` for a `configDependencies` entry
/// whose `packages:` row (or integrity) is missing.
pub(super) fn normalize_from_lockfile(
    env_lockfile: &EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<BTreeMap<String, NormalizedConfigDep>, ConfigDepError> {
    let mut deps = BTreeMap::new();
    let Some(importer) = env_lockfile.importers.get(EnvLockfile::ROOT_IMPORTER_KEY) else {
        return Ok(deps);
    };
    for (name, spec) in &importer.config_dependencies {
        let pkg_key = format!("{name}@{}", spec.version);
        let (key, pkg) = required_config_package(env_lockfile, &pkg_key)?;
        // Derive the tarball URL (when integrity-only) from the registry
        // that serves this package, honoring per-scope registry entries.
        let (integrity, tarball) = integrity_and_tarball(
            &pkg.resolution,
            name,
            &spec.version,
            opts.pick_registry(name),
        )
        .ok_or_else(|| ConfigDepError::EnvLockfileCorrupted {
            message: format!(
                r#"pnpm-lock.yaml is corrupted or incomplete: missing integrity for "{pkg_key}""#,
            ),
        })?;

        let optional_subdeps = snapshot_optional_subdeps(&key, name, env_lockfile, opts)?;

        deps.insert(
            name.clone(),
            NormalizedConfigDep {
                version: spec.version.clone(),
                integrity,
                tarball,
                optional_subdeps,
            },
        );
    }
    Ok(deps)
}

fn required_config_package<'a>(
    env_lockfile: &'a EnvLockfile,
    pkg_key: &str,
) -> Result<(pnpm_lockfile::PackageKey, &'a pnpm_lockfile::PackageMetadata), ConfigDepError> {
    let key = pkg_key.parse().map_err(|_| ConfigDepError::EnvLockfileCorrupted {
        message: format!(r#"pnpm-lock.yaml has an unparsable config-dependency key "{pkg_key}""#),
    })?;
    let pkg =
        env_lockfile.packages.get(&key).ok_or_else(|| ConfigDepError::EnvLockfileCorrupted {
            message: format!(
                "pnpm-lock.yaml is corrupted or incomplete: missing packages entry for \
                 \"{pkg_key}\" referenced from importers['.'].configDependencies",
            ),
        })?;
    Ok((key, pkg))
}

/// The normalized `optionalDependencies` of `key`'s snapshot, empty when
/// the lockfile records no snapshot for it or the snapshot declares none.
fn snapshot_optional_subdeps(
    key: &pnpm_lockfile::PackageKey,
    parent_name: &str,
    env_lockfile: &EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<Vec<NormalizedSubdep>, ConfigDepError> {
    let Some(snapshot) = env_lockfile.snapshots.get(key) else {
        return Ok(Vec::new());
    };
    let Some(optionals) = snapshot.optional_dependencies.as_ref() else {
        return Ok(Vec::new());
    };
    read_optional_subdeps(parent_name, optionals, env_lockfile, opts)
}

fn read_optional_subdeps(
    parent_name: &str,
    optionals: &std::collections::HashMap<pnpm_lockfile::PkgName, pnpm_lockfile::SnapshotDepRef>,
    env_lockfile: &EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<Vec<NormalizedSubdep>, ConfigDepError> {
    let mut subdeps = Vec::new();
    for (subdep_name, dep_ref) in optionals {
        let version = dep_ref.ver_peer().map(std::string::ToString::to_string).unwrap_or_default();
        let subdep_name = subdep_name.to_string();
        let subdep_key = format!("{subdep_name}@{version}");
        let key = subdep_key.parse().map_err(|_| ConfigDepError::EnvLockfileCorrupted {
            message: format!(r#"pnpm-lock.yaml has an unparsable subdep key "{subdep_key}""#),
        })?;
        let pkg = env_lockfile.packages.get(&key).ok_or_else(|| {
            ConfigDepError::EnvLockfileCorrupted {
                message: format!(
                    "pnpm-lock.yaml is corrupted or incomplete: missing packages entry for \
                     \"{subdep_key}\" referenced from optionalDependencies of config dependency \
                     \"{parent_name}\"",
                ),
            }
        })?;
        let (integrity, tarball) = integrity_and_tarball(
            &pkg.resolution,
            &subdep_name,
            &version,
            opts.pick_registry(&subdep_name),
        )
        .ok_or_else(|| ConfigDepError::EnvLockfileCorrupted {
            message: format!(
                "pnpm-lock.yaml is corrupted or incomplete: missing integrity for \
                         \"{subdep_key}\"",
            ),
        })?;
        subdeps.push(NormalizedSubdep {
            name: subdep_name.clone(),
            version,
            integrity,
            tarball,
            os: pkg.os.clone(),
            cpu: pkg.cpu.clone(),
            libc: pkg.libc.as_deref().map(<[String]>::to_vec),
        });
    }
    Ok(subdeps)
}

/// Extract `(integrity, tarball_url)` from a lockfile-form resolution,
/// deriving the canonical npm tarball URL when the registry resolution
/// omitted it.
fn integrity_and_tarball(
    resolution: &LockfileResolution,
    name: &str,
    version: &str,
    registry: &str,
) -> Option<(Integrity, String)> {
    match resolution {
        LockfileResolution::Registry(registry_resolution) => Some((
            registry_resolution.integrity.clone(),
            npm_tarball_url(name, version, TarballUrlOptions { registry, server_type: None }),
        )),
        LockfileResolution::Tarball(tarball) => {
            let integrity = tarball.integrity.clone()?;
            Some((integrity, tarball.tarball.clone()))
        }
        _ => None,
    }
}
