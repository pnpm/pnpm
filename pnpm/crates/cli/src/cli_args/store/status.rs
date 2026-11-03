//! `pacquet store status` — find packages something edited after they
//! were expanded out of the store.
//!
//! A package the store has no row for is skipped: there is nothing to
//! compare it against.

use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_deps_restorer::{
    AllowBuildPolicy, VirtualStoreLayout, safe_join_modules_dir, store_index_key_for_resolution,
    validate_importer_id, validate_virtual_store_slot_containment,
    virtual_store_layout_for_lockfile,
};
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::{AllowBuildValue, Host, Modules, read_modules_manifest};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_store_dir::{StoreIndex, package_dir_matches_index};
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Packages in the store have been mutated\nThese packages are modified:\n{}", modified.join("\n"))]
#[diagnostic(code(ERR_PNPM_MODIFIED_DEPENDENCY))]
pub struct ModifiedDependencyError {
    #[error(not(source))]
    pub modified: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "Cannot locate the installed package at {path}. Run \"pnpm install\" before checking store status."
)]
#[diagnostic(code(ERR_PNPM_STORE_STATUS_UNVERIFIABLE))]
struct UnverifiablePackageError {
    path: String,
}

struct PackageToCheck {
    dep_path: String,
    package_dir: PathBuf,
    store_index_key: String,
    global_virtual_store: bool,
}

pub(super) async fn run<Reporter: self::Reporter>(
    config: &'static Config,
    dir: &Path,
) -> miette::Result<()> {
    let lockfile_dir = config.lockfile_dir_for(dir).to_path_buf();
    let Some(lockfile) = Lockfile::load_wanted_from_dir(&lockfile_dir).into_diagnostic()? else {
        return report_untouched::<Reporter>(dir);
    };
    let modules_manifest =
        read_modules_manifest::<Host>(&project_modules_dir(config, &lockfile_dir))
            .into_diagnostic()?;
    let lockfile = if config.enable_global_virtual_store {
        Lockfile::load_current_from_install_state_dir(&config.install_state_dir)
            .into_diagnostic()?
            .unwrap_or(lockfile)
    } else {
        lockfile
    };
    let packages = packages_to_check(config, &lockfile, modules_manifest.as_ref(), &lockfile_dir)?;

    let store_dir = config.store_dir.root().to_path_buf();
    let frozen_store = config.frozen_store;
    let mut modified = tokio::task::spawn_blocking(move || {
        if config.enable_global_virtual_store {
            validate_project_links(config, &lockfile, &lockfile_dir, &packages)?;
        }
        find_modified(&store_dir, frozen_store, &packages)
    })
    .await
    .into_diagnostic()??;

    if modified.is_empty() {
        return report_untouched::<Reporter>(dir);
    }
    modified.sort_unstable();
    Err(ModifiedDependencyError { modified }.into())
}

/// Every installed package the store can verify, with where each lives.
///
/// Iterated from `snapshots:` rather than `packages:`: the snapshot key
/// carries the peer-dependency suffix that names the package's
/// materialized slot in the virtual store, and is the dep path
/// `.modules.yaml.skipped` records.
fn packages_to_check(
    config: &Config,
    lockfile: &Lockfile,
    modules_manifest: Option<&Modules>,
    lockfile_dir: &Path,
) -> miette::Result<Vec<PackageToCheck>> {
    let skipped: HashSet<&str> = modules_manifest
        .map(|manifest| {
            manifest.skipped
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let virtual_store_dir = match modules_manifest {
        Some(manifest) => PathBuf::from(&manifest.virtual_store_dir),
        None => resolve_virtual_store_dir(config, lockfile_dir),
    };

    let layout =
        installed_layout(config, lockfile, modules_manifest, lockfile_dir, &virtual_store_dir)?;
    Ok(lockfile.snapshots
        .iter()
        .flatten()
        .filter(|(key, _)| !skipped.contains(key.to_string().as_str()))
        .filter_map(|(key, _)| {
            let metadata = lockfile.packages
                .as_ref()?
                .get(&key.without_peer())?;
            let store_index_key =
                store_index_key_for_resolution(&metadata.resolution, &key.pkg_id(), true)?;
            let slot = layout.slot_dir(key);
            let modules_dir = virtual_store_dir
                .join(
                    slot.strip_prefix(layout.package_store_dir())
                        .expect("slot inside virtual store"),
                )
                .join("node_modules");
            Some(PackageToCheck {
                dep_path: key.to_string(),
                global_virtual_store: config.enable_global_virtual_store,
                package_dir: safe_join_modules_dir(&modules_dir, &key.name.to_string()).ok()?,
                store_index_key,
            })
        })
        .collect())
}

fn installed_layout(
    config: &Config,
    lockfile: &Lockfile,
    modules_manifest: Option<&Modules>,
    lockfile_dir: &Path,
    virtual_store_dir: &Path,
) -> miette::Result<VirtualStoreLayout> {
    let max_length = modules_manifest.map_or(config.virtual_store_dir_max_length, |manifest| {
        manifest.virtual_store_dir_max_length
    }) as usize;
    let layout = if config.enable_global_virtual_store {
        let policy = installed_build_policy(config, modules_manifest)?;
        virtual_store_layout_for_lockfile(
            config,
            config.node_version.as_deref(),
            lockfile,
            Some(&policy),
            Some(lockfile_dir),
        )
    } else {
        VirtualStoreLayout::legacy(virtual_store_dir, max_length)
    };
    validate_virtual_store_slot_containment(lockfile.snapshots.as_ref(), &layout)
        .into_diagnostic()?;
    Ok(layout)
}

fn installed_build_policy(
    config: &Config,
    modules_manifest: Option<&Modules>,
) -> miette::Result<AllowBuildPolicy> {
    let Some(manifest) = modules_manifest else {
        return AllowBuildPolicy::from_config(config).into_diagnostic();
    };
    let approvals = manifest.allow_builds
        .iter()
        .flatten()
        .filter_map(|(spec, value)| match value {
            AllowBuildValue::Bool(allowed) => Some((spec.clone(), *allowed)),
            AllowBuildValue::String(_) => None,
        })
        .collect();
    AllowBuildPolicy::from_approvals(&approvals, config.dangerously_allow_all_builds)
        .into_diagnostic()?
        .with_side_effects_cache_exclude(
            config.side_effects_cache_exclude.as_deref().unwrap_or_default(),
        )
        .into_diagnostic()
}

fn validate_project_links(
    config: &Config,
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    packages: &[PackageToCheck],
) -> miette::Result<()> {
    let package_dirs = resolved_package_dirs(packages)?;
    let root_modules_dir = project_modules_dir(config, lockfile_dir);
    for (id, importer) in &lockfile.importers {
        validate_importer_id(id.as_str())?;
        let modules_dir = project_modules_dir(config, &lockfile_dir.join(id.as_str()));
        let dependencies =
            [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies];
        for (alias, reference) in dependencies
            .into_iter()
            .flatten()
            .flatten()
        {
            let Some(key) = reference.version.resolved_key(alias) else { continue };
            let Some(expected) = package_dirs.get(key.to_string().as_str()) else { continue };
            let link = installed_project_link(&modules_dir, &root_modules_dir, &alias.to_string())?;
            let actual = installed_link_realpath(&link)?;
            if &actual != expected {
                return Err(UnverifiablePackageError { path: link.display().to_string() }.into());
            }
        }
    }
    Ok(())
}

fn resolved_package_dirs(packages: &[PackageToCheck]) -> miette::Result<HashMap<&str, PathBuf>> {
    packages
        .iter()
        .map(|package| {
            installed_link_realpath(&package.package_dir)
                .map(|path| (package.dep_path.as_str(), path))
        })
        .collect()
}

fn project_modules_dir(config: &Config, project_dir: &Path) -> PathBuf {
    let name = config
        .applies_package_configs()
        .then(|| pnpm_workspace::read_project_name(project_dir))
        .flatten();
    config.project_modules_dir(project_dir, name.as_deref())
}

fn installed_link_realpath(link: &Path) -> miette::Result<PathBuf> {
    match pnpm_fs::realpath_missing(link) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(UnverifiablePackageError { path: link.display().to_string() }.into())
        }
        result => result.into_diagnostic(),
    }
}

fn installed_project_link(
    modules_dir: &Path,
    root_modules_dir: &Path,
    alias: &str,
) -> miette::Result<PathBuf> {
    let link = safe_join_modules_dir(modules_dir, alias).into_diagnostic()?;
    match std::fs::symlink_metadata(&link) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            safe_join_modules_dir(root_modules_dir, alias).into_diagnostic()
        }
        Err(error) => Err(error).into_diagnostic(),
        Ok(_) => Ok(link),
    }
}

/// Re-hash every candidate against its store row. Runs on the blocking
/// pool: it is filesystem-bound work over the whole virtual store.
fn find_modified(
    store_dir: &Path,
    frozen_store: bool,
    packages: &[PackageToCheck],
) -> miette::Result<Vec<String>> {
    if !StoreIndex::exists(store_dir) {
        return Ok(Vec::new());
    }
    let store_index = if frozen_store {
        StoreIndex::open_immutable(store_dir).into_diagnostic()?
    } else {
        StoreIndex::open_readonly(store_dir).into_diagnostic()?
    };
    let keys = packages
        .iter()
        .map(|package| package.store_index_key.clone())
        .collect::<Vec<_>>();
    let rows = store_index.get_many(&keys).into_diagnostic()?;
    for package in packages {
        if package.global_virtual_store
            && rows.contains_key(&package.store_index_key)
            && !package.package_dir.try_exists().into_diagnostic()?
        {
            return Err(UnverifiablePackageError {
                path: package.package_dir.display().to_string(),
            }
            .into());
        }
    }
    Ok(packages
        .par_iter()
        .filter(|package| {
            rows.get(&package.store_index_key)
                .is_some_and(|row| !package_dir_matches_index(&package.package_dir, row))
        })
        .map(|package| package.dep_path.clone())
        .collect())
}

/// `.modules.yaml` records the virtual store it built, and is the source
/// of truth when it exists. Without one, the location an install would
/// record there is the best answer — relative to the lockfile directory,
/// which is where the setting is anchored when it is not spelled out.
fn resolve_virtual_store_dir(config: &Config, lockfile_dir: &Path) -> PathBuf {
    let virtual_store_dir = config.virtual_store_dir();
    if virtual_store_dir.is_absolute() {
        virtual_store_dir.to_path_buf()
    } else {
        lockfile_dir.join(virtual_store_dir)
    }
}

fn report_untouched<Reporter: self::Reporter>(dir: &Path) -> miette::Result<()> {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Packages in the store are untouched".to_string(),
        prefix: dir.display().to_string(),
    }));
    Ok(())
}
