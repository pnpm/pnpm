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

    let mut changed = false;
    let mut updated_catalogs = BTreeMap::new();
    for (catalog_name, entries) in lockfile_catalogs {
        let mut updated_entries = BTreeMap::new();
        for (alias, entry) in entries {
            let Some(specifier) = catalogs.get(catalog_name).and_then(|catalog| catalog.get(alias))
            else {
                if catalog_entry_is_referenced(lockfile, catalog_name, alias) {
                    return FastCatalogUpdate::Unsupported;
                }
                changed = true;
                continue;
            };
            if specifier == &entry.specifier {
                updated_entries.insert(alias.clone(), entry.clone());
                continue;
            }
            let (Ok(version), Ok(range)) =
                (Version::parse(&entry.version), Range::parse(specifier))
            else {
                return FastCatalogUpdate::Unsupported;
            };
            if !version.satisfies(&range) {
                return FastCatalogUpdate::Unsupported;
            }
            changed = true;
            updated_entries.insert(
                alias.clone(),
                pnpm_lockfile::ResolvedCatalogEntry {
                    specifier: specifier.clone(),
                    version: entry.version.clone(),
                },
            );
        }
        if !updated_entries.is_empty() {
            updated_catalogs.insert(catalog_name.clone(), updated_entries);
        }
    }
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
