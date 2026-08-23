//! `pacquet store status` — find packages something edited after they
//! were expanded out of the store.
//!
//! A package the store has no row for is skipped: there is nothing to
//! compare it against.

use derive_more::{Display, Error};
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_deps_restorer::{safe_join_modules_dir, store_index_key_for_resolution};
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::{Host, read_modules_manifest};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_store_dir::{StoreIndex, StoreIndexError, package_dir_matches_index};
use rayon::prelude::*;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// pnpm renders this as a title plus the list, so the message carries the
/// dep paths one per line and the remedy is the diagnostic's help.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Packages in the store have been mutated\nThese packages are modified:\n{}", modified.join("\n"))]
#[diagnostic(
    code(ERR_PNPM_MODIFIED_DEPENDENCY),
    help(r#"You can run "pnpm install --force" to refetch the modified packages"#)
)]
pub struct ModifiedDependencyError {
    #[error(not(source))]
    pub modified: Vec<String>,
}

struct PackageToCheck {
    dep_path: String,
    package_dir: PathBuf,
    store_index_key: String,
}

pub(super) async fn run<Reporter: self::Reporter>(
    config: &'static Config,
    dir: &Path,
) -> miette::Result<()> {
    let lockfile_dir = config.lockfile_dir_for(dir).to_path_buf();
    let Some(lockfile) = Lockfile::load_wanted_from_dir(&lockfile_dir).into_diagnostic()? else {
        return report_untouched::<Reporter>(dir);
    };
    let modules_dir = lockfile_dir.join("node_modules");
    let modules_manifest = read_modules_manifest::<Host>(&modules_dir).into_diagnostic()?;
    let skipped: HashSet<&str> = modules_manifest
        .as_ref()
        .map(|manifest| manifest.skipped.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let virtual_store_dir = modules_manifest.as_ref().map_or_else(
        || resolve_virtual_store_dir(config, &lockfile_dir),
        |manifest| PathBuf::from(&manifest.virtual_store_dir),
    );

    let max_length = config.virtual_store_dir_max_length as usize;
    let packages = lockfile
        .packages
        .iter()
        .flatten()
        .filter(|(key, _)| !skipped.contains(key.to_string().as_str()))
        .filter_map(|(key, metadata)| {
            let store_index_key =
                store_index_key_for_resolution(&metadata.resolution, &key.pkg_id(), true)?;
            let modules_dir =
                virtual_store_dir.join(key.to_virtual_store_name(max_length)).join("node_modules");
            Some(PackageToCheck {
                dep_path: key.to_string(),
                package_dir: safe_join_modules_dir(&modules_dir, &key.name.to_string()).ok()?,
                store_index_key,
            })
        })
        .collect::<Vec<_>>();

    let store_dir = config.store_dir.root().to_path_buf();
    let frozen_store = config.frozen_store;
    let mut modified =
        tokio::task::spawn_blocking(move || find_modified(&store_dir, frozen_store, &packages))
            .await
            .into_diagnostic()?
            .into_diagnostic()?;

    if modified.is_empty() {
        return report_untouched::<Reporter>(dir);
    }
    modified.sort_unstable();
    Err(ModifiedDependencyError { modified }.into())
}

/// Re-hash every candidate against its store row. Runs on the blocking
/// pool: it is filesystem-bound work over the whole virtual store.
fn find_modified(
    store_dir: &Path,
    frozen_store: bool,
    packages: &[PackageToCheck],
) -> Result<Vec<String>, StoreIndexError> {
    if !store_dir.join("index.db").exists() {
        return Ok(Vec::new());
    }
    let store_index = if frozen_store {
        StoreIndex::open_immutable(store_dir)?
    } else {
        StoreIndex::open_readonly(store_dir)?
    };
    let keys = packages.iter().map(|package| package.store_index_key.clone()).collect::<Vec<_>>();
    let rows = store_index.get_many(&keys)?;
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
/// of truth when it exists. Without one, the configured location is the
/// best answer — relative to the lockfile directory, which is where the
/// setting is anchored when it is not spelled out.
fn resolve_virtual_store_dir(config: &Config, lockfile_dir: &Path) -> PathBuf {
    if config.virtual_store_dir.is_absolute() {
        config.virtual_store_dir.clone()
    } else {
        lockfile_dir.join(&config.virtual_store_dir)
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
