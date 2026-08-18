use crate::fast_update_overrides::{
    FastOverride, RewriteContext, apply_rewrite_plan, build_replacement_plan,
};
use node_semver::{Range, Version};
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::{Lockfile, PkgName, ResolvedCatalogEntry};
use std::collections::BTreeMap;

/// Move a catalog entry to a version the lockfile does not have, without
/// resolving the whole graph.
///
/// Replacing the package is the rewrite an exact `pnpm.overrides` entry
/// performs, so this drives that machinery rather than repeating it;
/// [`crate::fast_update_catalogs`] keeps the range-only case, where the
/// specifier moves and the package does not.
///
/// An override moves a package everywhere it appears. A catalog entry
/// governs only the importers that reference it, so anything else
/// reaching the package would have to keep the old version while those
/// importers move — a graph holding both, which this cannot express.
pub(crate) async fn try_fast_update_catalog_versions(
    context: &RewriteContext<'_>,
    catalogs: &Catalogs,
) -> Option<Lockfile> {
    // The same gate the range-only path opens with: an importer pointing at
    // a catalog entry with nothing recorded for it needs the resolver, and
    // this path would otherwise never look at that entry.
    if !crate::fast_update_catalogs::catalog_references_have_snapshots(context.lockfile, catalogs) {
        return None;
    }
    let recorded = context.lockfile.catalogs.as_ref()?;
    let mut entries = Vec::new();
    let mut updated_catalogs = BTreeMap::new();
    for (catalog_name, recorded_entries) in recorded {
        let mut updated_entries = BTreeMap::new();
        for (alias, entry) in recorded_entries {
            let specifier = catalogs.get(catalog_name)?.get(alias)?;
            let locked = Version::parse(&entry.version).ok()?;
            if specifier == &entry.specifier {
                updated_entries.insert(alias.clone(), entry.clone());
                continue;
            }
            // A specifier the locked version still satisfies moves nothing
            // but the specifier, exactly as the range-only path would.
            if Range::parse(specifier).is_ok_and(|range| locked.satisfies(&range)) {
                updated_entries.insert(
                    alias.clone(),
                    ResolvedCatalogEntry {
                        specifier: specifier.clone(),
                        version: entry.version.clone(),
                    },
                );
                continue;
            }
            let wanted = Version::parse(specifier).ok()?;
            let name = PkgName::parse(alias).ok()?;
            if !catalog_entry_is_sole_reference(context.lockfile, catalog_name, &name) {
                return None;
            }
            entries.push(FastOverride {
                name,
                new_version: Some(wanted.clone()),
                old_version: Some(locked),
                parent: None,
            });
            updated_entries.insert(
                alias.clone(),
                ResolvedCatalogEntry { specifier: specifier.clone(), version: wanted.to_string() },
            );
        }
        updated_catalogs.insert(catalog_name.clone(), updated_entries);
    }
    if entries.is_empty() {
        return None;
    }

    let plan = build_replacement_plan(context.lockfile, entries)?;
    let mut updated = apply_rewrite_plan(context, &plan).await?;
    updated.catalogs = Some(updated_catalogs);
    Some(updated)
}

/// Whether the catalog entry is the only thing in the lockfile that
/// reaches `name`.
fn catalog_entry_is_sole_reference(
    lockfile: &Lockfile,
    catalog_name: &str,
    name: &PkgName,
) -> bool {
    let importers_agree = lockfile.importers.values().all(|importer| {
        [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        .all(|dependencies| {
            dependencies.get(name).is_none_or(|dependency| {
                pnpm_catalogs_protocol_parser::parse_catalog_protocol(&dependency.specifier)
                    == Some(catalog_name)
            })
        })
    });
    let no_package_depends_on_it = lockfile.snapshots.as_ref().is_none_or(|snapshots| {
        snapshots.values().all(|snapshot| {
            [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
                .into_iter()
                .flatten()
                .all(|dependencies| !dependencies.contains_key(name))
        })
    });
    importers_agree && no_package_depends_on_it
}

#[cfg(test)]
mod tests;
