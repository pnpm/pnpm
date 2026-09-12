use node_semver::{Range, Version};
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{Lockfile, PkgName};
use std::collections::BTreeMap;

pub(crate) enum FastCatalogUpdate {
    Unchanged,
    Updated(Box<Lockfile>),
    Unsupported,
}

pub(crate) fn try_fast_update_catalogs(
    lockfile: &Lockfile,
    catalogs: &Catalogs,
    overrides_use_catalogs: bool,
) -> FastCatalogUpdate {
    if !catalog_references_have_snapshots(lockfile, catalogs) {
        return FastCatalogUpdate::Unsupported;
    }
    let Some(lockfile_catalogs) = lockfile.catalogs.as_ref() else {
        return if catalogs.is_empty() {
            FastCatalogUpdate::Unchanged
        } else {
            FastCatalogUpdate::Unsupported
        };
    };

    let Some((updated_catalogs, changed)) =
        retargeted_catalogs(lockfile, catalogs, lockfile_catalogs)
    else {
        return FastCatalogUpdate::Unsupported;
    };
    if !changed {
        return FastCatalogUpdate::Unchanged;
    }
    if overrides_use_catalogs {
        return FastCatalogUpdate::Unsupported;
    }

    let mut candidate = lockfile.clone();
    candidate.catalogs = (!updated_catalogs.is_empty()).then_some(updated_catalogs);
    FastCatalogUpdate::Updated(Box::new(candidate))
}

/// The lockfile's recorded catalogs: resolved entries per catalog name.
type LockfileCatalogs = BTreeMap<String, BTreeMap<String, pnpm_lockfile::ResolvedCatalogEntry>>;

/// The catalogs the lockfile would record after the retarget, and whether
/// anything moved. `None` when an entry needs the resolver.
fn retargeted_catalogs(
    lockfile: &Lockfile,
    catalogs: &Catalogs,
    lockfile_catalogs: &LockfileCatalogs,
) -> Option<(LockfileCatalogs, bool)> {
    let mut changed = false;
    let mut updated_catalogs = BTreeMap::new();
    for (catalog_name, entries) in lockfile_catalogs {
        let mut updated_entries = BTreeMap::new();
        for (alias, entry) in entries {
            match retarget_catalog_entry(lockfile, catalogs, catalog_name, alias, entry)? {
                CatalogEntryUpdate::Dropped => changed = true,
                CatalogEntryUpdate::Kept(entry) => {
                    updated_entries.insert(alias.clone(), entry);
                }
                CatalogEntryUpdate::Retargeted(entry) => {
                    changed = true;
                    updated_entries.insert(alias.clone(), entry);
                }
            }
        }
        if !updated_entries.is_empty() {
            updated_catalogs.insert(catalog_name.clone(), updated_entries);
        }
    }
    Some((updated_catalogs, changed))
}

/// What the workspace's catalogs do to one recorded catalog entry.
enum CatalogEntryUpdate {
    /// The specifier is unchanged.
    Kept(pnpm_lockfile::ResolvedCatalogEntry),
    /// The specifier moved but the locked version still satisfies it.
    Retargeted(pnpm_lockfile::ResolvedCatalogEntry),
    /// The workspace no longer declares the entry and nothing references it.
    Dropped,
}

/// `None` when the entry needs a resolution: it is still referenced but no
/// longer declared, or its locked version cannot satisfy the new specifier.
fn retarget_catalog_entry(
    lockfile: &Lockfile,
    catalogs: &Catalogs,
    catalog_name: &str,
    alias: &str,
    entry: &pnpm_lockfile::ResolvedCatalogEntry,
) -> Option<CatalogEntryUpdate> {
    let Some(specifier) = catalogs.get(catalog_name).and_then(|catalog| catalog.get(alias)) else {
        if catalog_entry_is_referenced(lockfile, catalog_name, alias) {
            return None;
        }
        return Some(CatalogEntryUpdate::Dropped);
    };
    if specifier == &entry.specifier {
        return Some(CatalogEntryUpdate::Kept(entry.clone()));
    }
    let (Ok(version), Ok(range)) = (Version::parse(&entry.version), Range::parse(specifier)) else {
        return None;
    };
    if !version.satisfies(&range) {
        return None;
    }
    Some(CatalogEntryUpdate::Retargeted(pnpm_lockfile::ResolvedCatalogEntry {
        specifier: specifier.clone(),
        version: entry.version.clone(),
    }))
}

pub(crate) fn catalog_references_have_snapshots(lockfile: &Lockfile, catalogs: &Catalogs) -> bool {
    lockfile.importers.values().all(|importer| {
        [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        .flat_map(|dependencies| dependencies.iter())
        .all(|(alias, dependency)| {
            let Some(catalog_name) = dependency.specifier.strip_prefix("catalog:") else {
                return true;
            };
            let catalog_name = if catalog_name.is_empty() { "default" } else { catalog_name };
            let alias = alias.to_string();
            catalogs.get(catalog_name).and_then(|catalog| catalog.get(&alias)).is_some()
                && lockfile
                    .catalogs
                    .as_ref()
                    .and_then(|catalogs| catalogs.get(catalog_name))
                    .and_then(|catalog| catalog.get(&alias))
                    .is_some()
        })
    })
}

pub(crate) fn catalog_entry_is_referenced(
    lockfile: &Lockfile,
    catalog_name: &str,
    alias: &str,
) -> bool {
    let Ok(alias) = PkgName::parse(alias) else { return true };
    // Parsed rather than compared to a rebuilt protocol string, so the
    // `catalog:default` spelling of the default catalog counts too.
    lockfile.importers.values().any(|importer| {
        [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|dependencies| {
            dependencies.get(&alias).is_some_and(|dependency| {
                pnpm_catalogs_protocol_parser::parse_catalog_protocol(&dependency.specifier)
                    == Some(catalog_name)
            })
        })
    })
}

#[cfg(test)]
mod tests;
