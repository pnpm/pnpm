//! The version an importer's specifier implies in the next lockfile format,
//! where a dependency can be written as its specifier alone.

use crate::{PackageKey, VersionPart};
use std::collections::HashMap;

/// The path of each injected workspace package, by name: the `<path>` of
/// its `name@file:<path>` entry in `packages`. A name with several such
/// entries has none here, since its path would not follow from the name.
pub(crate) type InjectedPaths = HashMap<String, String>;

pub(crate) fn injected_paths<'a>(keys: impl Iterator<Item = &'a PackageKey>) -> InjectedPaths {
    let mut paths: HashMap<String, Option<String>> = HashMap::new();
    for key in keys {
        let VersionPart::File(path) = key.suffix.version() else { continue };
        paths
            .entry(key.name.to_string())
            .and_modify(|entry| *entry = None)
            .or_insert_with(|| Some(path.clone()));
    }
    paths
        .into_iter()
        .filter_map(|(name, path)| Some((name, path?)))
        .collect()
}

/// The version `specifier` implies for `alias`, or `None` when it implies
/// none, as for a range. `catalog_entry(catalog, alias)` is the specifier
/// and version the `catalogs` section records for a `catalog:` specifier,
/// and `injected_path(name)` the path of the injected workspace package
/// `name`, from [`InjectedPaths`].
///
/// An exact version implies itself, `npm:name@1.2.3` implies `name@1.2.3`,
/// a `link:` or `file:` path implies itself, `catalog:` implies the
/// catalog entry's version, as `name@version` when the entry is an `npm:`
/// alias, and `workspace:` implies `file:<path>` when the package is
/// injected, as `name@file:<path>` when the specifier names the package.
pub(crate) fn implied_version<'path>(
    alias: &str,
    specifier: &str,
    catalog_entry: impl FnOnce(&str, &str) -> Option<(String, String)>,
    injected_path: impl FnOnce(&str) -> Option<&'path str>,
) -> Option<String> {
    if let Some(target) = specifier.strip_prefix("workspace:") {
        let name = workspace_name(target);
        let path = injected_path(name.unwrap_or(alias))?;
        return Some(match name {
            Some(name) if name != alias => format!("{name}@file:{path}"),
            _ => format!("file:{path}"),
        });
    }
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

/// The version a catalog entry's specifier implies: the specifier itself, or
/// the version of an `npm:name@1.2.3` alias, since a catalog entry records
/// the alias's bare version. `None` for `npm:name` without a version.
pub(crate) fn catalog_implied_version(specifier: &str) -> Option<&str> {
    let Some(target) = specifier.strip_prefix("npm:") else { return Some(specifier) };
    target.get(alias_name(target).len() + 1..)
}

/// The package name of an `npm:` alias target, `name` or `name@range`.
fn alias_name(target: &str) -> &str {
    match target[1..].find('@') {
        Some(at) => &target[..=at],
        None => target,
    }
}

/// The package a `workspace:` specifier names, in its `workspace:name@range`
/// form; `None` for a bare range such as `*` or `^1.0.0`.
fn workspace_name(target: &str) -> Option<&str> {
    let at = target.get(1..)?.find('@')? + 1;
    Some(&target[..at])
}
