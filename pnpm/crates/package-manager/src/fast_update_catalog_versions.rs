use crate::fast_update_overrides::{
    FastOverride, RewriteContext, apply_rewrite_plan, build_replacement_plan,
};
use node_semver::Version;
use pacquet_catalogs_types::Catalogs;
use pacquet_lockfile::{Lockfile, PkgName, ResolvedCatalogEntry};
use std::collections::BTreeMap;

/// Move a catalog entry to a version the lockfile does not have, without
/// resolving the whole graph.
///
/// [`crate::fast_update_catalogs`] handles the range-only case, where the
/// recorded version still satisfies the new specifier and nothing but the
/// specifier moves. This handles the other half: the entry now names a
/// version the locked one cannot satisfy, so the package itself has to be
/// replaced. That is the same rewrite an exact `pnpm.overrides` entry
/// performs, so it reuses that machinery — including the check that every
/// locked child still satisfies the new version's manifest.
///
/// Unlike an override, a catalog entry only governs the importers that
/// reference it. `None` when anything else reaches the package, since the
/// graph would then have to hold both versions.
pub(crate) async fn try_fast_update_catalog_versions(
    context: &RewriteContext<'_>,
    catalogs: &Catalogs,
) -> Option<Lockfile> {
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
            // A specifier the locked version still satisfies belongs to
            // the range-only path, which rewrites nothing.
            let wanted = Version::parse(specifier).ok()?;
            if wanted == locked {
                return None;
            }
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
/// reaches `name`: every importer that depends on it does so through this
/// catalog, and no package depends on it at all.
///
/// An override moves a package everywhere it appears. A catalog entry
/// cannot, so anything else pointing at the package would have to keep the
/// old version while the catalog's importers move to the new one, leaving
/// the graph holding both.
fn catalog_entry_is_sole_reference(
    lockfile: &Lockfile,
    catalog_name: &str,
    name: &PkgName,
) -> bool {
    let protocol = if catalog_name == "default" {
        "catalog:".to_string()
    } else {
        format!("catalog:{catalog_name}")
    };
    let importers_agree = lockfile.importers.values().all(|importer| {
        [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        .all(|dependencies| {
            dependencies.get(name).is_none_or(|dependency| dependency.specifier == protocol)
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
