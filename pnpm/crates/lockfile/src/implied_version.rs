//! The version an importer's specifier implies in the next lockfile format,
//! where a dependency can be written as its specifier alone.

/// The version `specifier` implies for `alias`, or `None` when it implies
/// none, as for a range. `catalog_entry(catalog, alias)` is the specifier
/// and version the `catalogs` section records for a `catalog:` specifier.
///
/// An exact version implies itself, `npm:name@1.2.3` implies `name@1.2.3`,
/// a `link:` or `file:` path implies itself, and `catalog:` implies the
/// catalog entry's version, as `name@version` when the entry is an `npm:`
/// alias.
pub(crate) fn implied_version(
    alias: &str,
    specifier: &str,
    catalog_entry: impl FnOnce(&str, &str) -> Option<(String, String)>,
) -> Option<String> {
    if let Some(catalog) = specifier.strip_prefix("catalog:") {
        let catalog = if catalog.is_empty() { "default" } else { catalog };
        let (catalog_specifier, version) = catalog_entry(catalog, alias)?;
        return Some(match catalog_specifier.strip_prefix("npm:") {
            Some(target) => format!("{}@{version}", alias_name(target)),
            None => version,
        });
    }
    Some(
        specifier
            .strip_prefix("npm:")
            .unwrap_or(specifier)
            .to_string(),
    )
}

/// The package name of an `npm:` alias target, `name` or `name@range`.
fn alias_name(target: &str) -> &str {
    match target[1..].find('@') {
        Some(at) => &target[..=at],
        None => target,
    }
}
