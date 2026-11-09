//! The projects that `workspace:` dependencies link to, by package name.
//!
//! An importer is keyed by its path, and a linked dependency's version is
//! `link:<path>` relative to the referrer. A project that a `workspace:`
//! dependency links to records its `name`, so the dependency can be its
//! specifier alone and the reader can find the link's target among the
//! importers.

use super::NextImporter;
use crate::{
    ImporterDepVersion,
    implied_version::{link_target, workspace_name},
};
use std::collections::{HashMap, HashSet};

/// Each linked project's importer id, by package name. A name that two
/// importers claim, or an importer that two names claim, has no entry.
pub(crate) type LinkedImporters = HashMap<String, String>;

/// The projects `workspace:` dependencies link to, named by the dependency
/// that links them: its alias, or the name a `workspace:name@range`
/// specifier gives. A link to a path that is not an importer is left out.
pub(super) fn linked_importers(importers: &HashMap<String, NextImporter>) -> LinkedImporters {
    let mut linked = HashMap::new();
    for (id, importer) in importers {
        for (alias, dep) in importer.resolved_dependencies() {
            let Some(target) = dep.specifier.strip_prefix("workspace:") else { continue };
            let ImporterDepVersion::Link(link) = &dep.version else { continue };
            let Some(target_id) = link_target(id, link) else { continue };
            if !importers.contains_key(&target_id) {
                continue;
            }
            let name = workspace_name(target).map_or_else(|| alias.to_string(), str::to_string);
            claim(&mut linked, name, target_id);
        }
    }
    one_to_one(linked)
}

/// Each named importer's id, from the `name` the importers record.
pub(super) fn named_importers(importers: &HashMap<String, NextImporter>) -> LinkedImporters {
    let mut named = HashMap::new();
    for (id, importer) in importers {
        if let Some(name) = &importer.name {
            claim(&mut named, name.clone(), id.clone());
        }
    }
    one_to_one(named)
}

/// The name each importer in `linked` records, by importer id.
pub(super) fn names(linked: &LinkedImporters) -> HashMap<&str, &str> {
    linked
        .iter()
        .map(|(name, id)| (id.as_str(), name.as_str()))
        .collect()
}

/// Records that `name` is the importer `id`; a second, different claim
/// voids the name.
fn claim(claims: &mut HashMap<String, Option<String>>, name: String, id: String) {
    match claims.entry(name) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if entry.get().as_deref() != Some(id.as_str()) {
                entry.insert(None);
            }
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(Some(id));
        }
    }
}

/// The claims that stand, with every importer that two names claim dropped,
/// since an importer records one name.
fn one_to_one(claims: HashMap<String, Option<String>>) -> LinkedImporters {
    let by_name: LinkedImporters = claims
        .into_iter()
        .filter_map(|(name, id)| Some((name, id?)))
        .collect();
    let mut seen = HashSet::new();
    let mut shared = HashSet::new();
    for id in by_name.values() {
        if !seen.insert(id.as_str()) {
            shared.insert(id.clone());
        }
    }
    by_name
        .into_iter()
        .filter(|(_, id)| !shared.contains(id))
        .collect()
}
