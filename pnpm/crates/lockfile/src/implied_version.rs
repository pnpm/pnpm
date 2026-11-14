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
/// `injected_path(name)` the path of the injected workspace package `name`,
/// from [`InjectedPaths`], and `link_path(name)` the `link:` path from the
/// referrer to the project `name`, from the linked importers.
///
/// An exact version implies itself, `npm:name@1.2.3` implies `name@1.2.3`,
/// a `link:` or `file:` path implies itself, `catalog:` implies the
/// catalog entry's version, as `name@version` when the entry is an `npm:`
/// alias, and `workspace:` implies `file:<path>` when the package is
/// injected, as `name@file:<path>` when the specifier names the package,
/// or `link:<path>` when it is linked. A package that is both injected
/// and linked implies nothing.
pub(crate) fn implied_version<'path>(
    alias: &str,
    specifier: &str,
    catalog_entry: impl FnOnce(&str, &str) -> Option<(String, String)>,
    injected_path: impl FnOnce(&str) -> Option<&'path str>,
    link_path: impl FnOnce(&str) -> Option<String>,
) -> Option<String> {
    if let Some(target) = specifier.strip_prefix("workspace:") {
        return workspace_version(alias, target, injected_path, link_path);
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
    match pnpm_deps_path::version_separator_index(target) {
        Some(at) => &target[..at],
        None => target,
    }
}

/// The version a `workspace:` specifier with `target` after the scheme
/// implies, as [`implied_version`] says.
fn workspace_version<'path>(
    alias: &str,
    target: &str,
    injected_path: impl FnOnce(&str) -> Option<&'path str>,
    link_path: impl FnOnce(&str) -> Option<String>,
) -> Option<String> {
    let name = workspace_name(target);
    let package = name.unwrap_or(alias);
    match (injected_path(package), link_path(package)) {
        (Some(path), None) => Some(match name {
            Some(name) if name != alias => format!("{name}@file:{path}"),
            _ => format!("file:{path}"),
        }),
        (None, Some(path)) => Some(format!("link:{path}")),
        _ => None,
    }
}

/// The package a `workspace:` specifier names, in its `workspace:name@range`
/// form; `None` for a bare range such as `*` or `^1.0.0`.
pub(crate) fn workspace_name(target: &str) -> Option<&str> {
    let at = pnpm_deps_path::version_separator_index(target)?;
    Some(&target[..at])
}

/// The importer that `link`, relative to the importer `id`, points to, as
/// an importer id: a path from the workspace root, or `.` for the root.
/// `None` when the link leaves the workspace.
pub(crate) fn link_target(id: &str, link: &str) -> Option<String> {
    let mut path: Vec<&str> = segments(id).collect();
    for segment in link.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                path.pop()?;
            }
            segment => path.push(segment),
        }
    }
    Some(joined(&path))
}

/// The `link:` path from the importer `id` to the importer `target`.
pub(crate) fn link_path(id: &str, target: &str) -> String {
    let from: Vec<&str> = segments(id).collect();
    let to: Vec<&str> = segments(target).collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(from, to)| from == to)
        .count();
    let path: Vec<&str> = std::iter::repeat_n("..", from.len() - common)
        .chain(to[common..].iter().copied())
        .collect();
    joined(&path)
}

fn segments(id: &str) -> impl Iterator<Item = &str> {
    id.split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
}

fn joined(segments: &[&str]) -> String {
    if segments.is_empty() { ".".to_string() } else { segments.join("/") }
}

#[cfg(test)]
mod tests;
