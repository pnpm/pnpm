use super::{
    Catalogs, IndexMap, Manifest, mapping_keys, remove_mapping_entries, remove_top_level_block,
    upsert,
};

/// Merge `updated` into `manifest`'s catalog blocks. Returns whether anything
/// changed.
pub(crate) fn add_catalogs(
    manifest: &mut Manifest,
    updated: &Catalogs,
) -> Result<bool, Box<yamlpatch::Error>> {
    let mut changed = false;
    for (catalog_name, entries) in updated {
        if entries.is_empty() {
            continue;
        }
        for (dep, specifier) in entries {
            changed |= upsert(manifest, catalog_name, dep, specifier)?;
        }
    }
    Ok(changed)
}

/// Raw dependency specifiers per package name, collected from every
/// workspace project manifest plus the workspace manifest's own
/// `overrides:` values. The upstream `packageReferences` map: a catalog
/// entry survives the cleanup pass only when this map holds a
/// `catalog:` reference for its package.
pub(crate) type CatalogReferences =
    std::collections::BTreeMap<String, std::collections::BTreeSet<String>>;

/// The `catalogPrune` pass: drop catalog entries that no
/// collected reference names. A default-catalog entry survives only via
/// a bare `catalog:` reference; a named-catalog entry survives via
/// `catalog:<name>` or bare `catalog:`. Emptied blocks are dropped
/// whole. Returns whether anything changed.
pub(crate) fn remove_unused_catalogs(
    manifest: &mut Manifest,
    references: &CatalogReferences,
) -> bool {
    remove_unused_default_catalog(manifest, references)
        | remove_unused_named_catalogs(manifest, references)
}

fn is_referenced(references: &CatalogReferences, pkg: &str, specs: &[&str]) -> bool {
    references.get(pkg).is_some_and(|refs| specs.iter().any(|spec| refs.contains(*spec)))
}

fn remove_unused_default_catalog(manifest: &mut Manifest, references: &CatalogReferences) -> bool {
    const BLOCK: &str = "catalog";
    let Some(catalog) = manifest.catalog.as_ref() else { return false };
    let to_remove: Vec<String> = catalog
        .keys()
        .filter(|pkg| !is_referenced(references, pkg, &["catalog:"]))
        .cloned()
        .collect();
    if to_remove.len() == catalog.len() {
        manifest.set_text(remove_top_level_block(manifest.text(), BLOCK));
        manifest.catalog = None;
        manifest.top_level_keys.retain(|key| key != BLOCK);
        return true;
    }
    if to_remove.is_empty() || !has_removable_entries(manifest.text(), &[BLOCK]) {
        return false;
    }
    manifest.set_text(remove_mapping_entries(manifest.text(), &[BLOCK], &to_remove));
    let catalog = manifest.catalog.as_mut().expect("catalog presence checked above");
    for pkg in &to_remove {
        catalog.shift_remove(pkg);
    }
    true
}

fn remove_unused_named_catalogs(manifest: &mut Manifest, references: &CatalogReferences) -> bool {
    const BLOCK: &str = "catalogs";
    let Some(catalogs) = manifest.catalogs.as_ref() else { return false };

    let (names_to_drop, entry_removals) = unreferenced_catalog_entries(catalogs, references);

    let mut changed = false;
    for (name, to_remove) in &entry_removals {
        changed |= remove_catalog_entries(manifest, BLOCK, name, to_remove);
    }

    let total_names = manifest.catalogs.as_ref().map_or(0, IndexMap::len);
    if names_to_drop.len() == total_names {
        manifest.set_text(remove_top_level_block(manifest.text(), BLOCK));
        manifest.catalogs = None;
        manifest.top_level_keys.retain(|key| key != BLOCK);
        return true;
    }
    if !names_to_drop.is_empty() && has_removable_entries(manifest.text(), &[BLOCK]) {
        manifest.set_text(remove_mapping_entries(manifest.text(), &[BLOCK], &names_to_drop));
        let catalogs = manifest.catalogs.as_mut().expect("catalogs presence checked above");
        for name in &names_to_drop {
            catalogs.shift_remove(name);
        }
        changed = true;
    }
    changed
}

/// The named catalogs no project references at all, and the per-catalog
/// entries to drop from the ones still in use.
fn unreferenced_catalog_entries(
    catalogs: &IndexMap<String, IndexMap<String, String>>,
    references: &CatalogReferences,
) -> (Vec<String>, Vec<(String, Vec<String>)>) {
    let mut names_to_drop: Vec<String> = Vec::new();
    let mut entry_removals: Vec<(String, Vec<String>)> = Vec::new();
    for (name, entries) in catalogs {
        let scoped = format!("catalog:{name}");
        let to_remove: Vec<String> = entries
            .keys()
            .filter(|pkg| !is_referenced(references, pkg, &[scoped.as_str(), "catalog:"]))
            .cloned()
            .collect();
        if to_remove.len() == entries.len() {
            names_to_drop.push(name.clone());
        } else if !to_remove.is_empty() {
            entry_removals.push((name.clone(), to_remove));
        }
    }
    (names_to_drop, entry_removals)
}

fn remove_catalog_entries(
    manifest: &mut Manifest,
    block: &str,
    name: &str,
    to_remove: &[String],
) -> bool {
    if !has_removable_entries(manifest.text(), &[block, name]) {
        return false;
    }
    manifest.set_text(remove_mapping_entries(manifest.text(), &[block, name], to_remove));
    let entries = manifest
        .catalogs
        .as_mut()
        .and_then(|catalogs| catalogs.get_mut(name))
        .expect("named catalog presence checked above");
    for pkg in to_remove {
        entries.shift_remove(pkg);
    }
    true
}

/// Whether the mapping at `path` holds entries the removal splices can
/// excise: per-line entries in block style, or the entries of a
/// single-line flow mapping.
fn has_removable_entries(text: &str, path: &[&str]) -> bool {
    !mapping_keys(text, path).is_empty()
}
