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
            match catalog_version_update(context.lockfile, catalogs, catalog_name, alias, entry)? {
                CatalogVersionUpdate::Unmoved(entry) => {
                    updated_entries.insert(alias.clone(), entry);
                }
                CatalogVersionUpdate::Bumped { entry, bump } => {
                    entries.push(*bump);
                    updated_entries.insert(alias.clone(), entry);
                }
            }
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

/// What the workspace's catalogs do to one recorded catalog entry.
enum CatalogVersionUpdate {
    /// The locked version stands; only the recorded specifier may differ.
    Unmoved(ResolvedCatalogEntry),
    /// The entry moves to a new version, which every reference has to follow.
    Bumped { entry: ResolvedCatalogEntry, bump: Box<FastOverride> },
}

/// `None` when the entry needs a resolution: it is no longer declared, its
/// specifier is not an exact version, or something other than this catalog
/// entry reaches the package.
fn catalog_version_update(
    lockfile: &Lockfile,
    catalogs: &Catalogs,
    catalog_name: &str,
    alias: &str,
    entry: &ResolvedCatalogEntry,
) -> Option<CatalogVersionUpdate> {
    let specifier = catalogs.get(catalog_name)?.get(alias)?;
    let locked = Version::parse(&entry.version).ok()?;
    if specifier == &entry.specifier {
        return Some(CatalogVersionUpdate::Unmoved(entry.clone()));
    }
    // A specifier the locked version still satisfies moves nothing
    // but the specifier, exactly as the range-only path would.
    if Range::parse(specifier).is_ok_and(|range| locked.satisfies(&range)) {
        return Some(CatalogVersionUpdate::Unmoved(ResolvedCatalogEntry {
            specifier: specifier.clone(),
            version: entry.version.clone(),
        }));
    }
    let wanted = Version::parse(specifier).ok()?;
    let name = PkgName::parse(alias).ok()?;
    if !catalog_entry_is_sole_reference(lockfile, catalog_name, &name) {
        return None;
    }
    Some(CatalogVersionUpdate::Bumped {
        entry: ResolvedCatalogEntry { specifier: specifier.clone(), version: wanted.to_string() },
        bump: Box::new(FastOverride {
            name,
            new_version: Some(wanted),
            old_version: Some(locked),
            parent: None,
        }),
    })
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
