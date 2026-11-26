//! Turns a manifest into a lockfile.

use crate::{
    binary_cache::{
        BinaryCache,
        CacheError,
    },
    catalog::{
        Catalog,
        CatalogError,
        CatalogPackage,
    },
    closure::resolve_closure,
    lockfile::{
        LOCKFILE_VERSION,
        LockedPackage,
        LockedSystem,
        Lockfile,
    },
    manifest::Manifest,
};
use derive_more::{
    Display,
    Error,
};
use futures_util::future::try_join_all;
use miette::Diagnostic;
use pnpz_nix_store::StorePath;
use std::collections::BTreeMap;

#[derive(Debug, Display, Error, Diagnostic)]
pub enum LockError {
    #[diagnostic(transparent)]
    Catalog(CatalogError),
    #[diagnostic(transparent)]
    Cache(CacheError),
    #[display("{name}@{version} has no build for {system}")]
    #[diagnostic(code(ERR_PNPZ_NO_BUILD_FOR_SYSTEM))]
    NoBuildForSystem { name: String, version: String, system: String },
}

/// The systems a manifest locks: the ones it lists, or `host_system` when it
/// lists none.
#[must_use]
pub fn wanted_systems(manifest: &Manifest, host_system: &str) -> Vec<String> {
    if manifest.systems.is_empty() {
        return vec![host_system.to_string()];
    }
    manifest.systems.clone()
}

/// Whether `lockfile` already answers `manifest` for every one of `systems`.
#[must_use]
pub fn is_up_to_date(manifest: &Manifest, lockfile: &Lockfile, systems: &[String]) -> bool {
    let wanted = manifest.packages();
    wanted.len() == lockfile.packages.len()
        && wanted
            .iter()
            .all(|(name, specifier)| {
                lockfile.packages
                    .get(name)
                    .is_some_and(|locked| is_reusable(locked, specifier, systems))
            })
        && systems
            .iter()
            .all(|system| lockfile.closures.contains_key(system))
}

fn is_reusable(locked: &LockedPackage, specifier: &str, systems: &[String]) -> bool {
    locked.specifier == specifier
        && systems
            .iter()
            .all(|system| locked.systems.contains_key(system))
}

/// Resolves `manifest` for `systems`.
///
/// A package whose entry in `previous` still matches the manifest keeps its
/// locked version. Every other package is resolved by the catalog, and the
/// closures are rebuilt from the binary cache.
pub async fn lock(
    manifest: &Manifest,
    previous: Option<&Lockfile>,
    systems: &[String],
    (catalog, cache): (&Catalog, &BinaryCache),
) -> Result<Lockfile, LockError> {
    let packages = try_join_all(
        manifest
            .packages()
            .into_iter()
            .map(|(name, specifier)| async {
                let reusable = previous
                    .and_then(|lockfile| lockfile.packages.get(&name))
                    .filter(|locked| is_reusable(locked, &specifier, systems));
                let package = if let Some(locked) = reusable {
                    locked.clone()
                } else {
                    let resolved =
                        catalog.resolve(&name, &specifier).await.map_err(LockError::Catalog)?;
                    locked_package(&name, specifier, resolved, systems)?
                };
                Ok::<_, LockError>((name, package))
            }),
    )
    .await?;
    let mut lockfile = Lockfile {
        lockfile_version: LOCKFILE_VERSION,
        packages: packages.into_iter().collect(),
        closures: BTreeMap::new(),
    };
    for system in systems {
        let roots: Vec<StorePath> = lockfile
            .environment_paths(system)
            .into_iter()
            .cloned()
            .collect();
        let closure = resolve_closure(cache, &roots).await.map_err(LockError::Cache)?;
        lockfile.closures.insert(system.clone(), closure);
    }
    Ok(lockfile)
}

fn locked_package(
    name: &str,
    specifier: String,
    resolved: CatalogPackage,
    systems: &[String],
) -> Result<LockedPackage, LockError> {
    let CatalogPackage { version, systems: mut available } = resolved;
    let locked_systems = systems
        .iter()
        .map(|system| match available.remove(system) {
            Some(found) => {
                let locked = LockedSystem {
                    nixpkgs: found.nixpkgs,
                    attr: found.attr,
                    outputs: found.outputs,
                };
                Ok((system.clone(), locked))
            }
            None => Err(LockError::NoBuildForSystem {
                name: name.to_string(),
                version: version.clone(),
                system: system.clone(),
            }),
        })
        .collect::<Result<_, _>>()?;
    Ok(LockedPackage { specifier, version, systems: locked_systems })
}
