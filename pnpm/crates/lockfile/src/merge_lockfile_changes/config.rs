use crate::{CatalogSnapshots, LockfileSettings, ResolvedCatalogEntry};
use indexmap::IndexMap;
use std::collections::BTreeMap;

use super::{Winner, take_changed, winner};

pub(super) fn merge_settings(
    ours: Option<&LockfileSettings>,
    theirs: Option<&LockfileSettings>,
) -> Option<LockfileSettings> {
    match (ours, theirs) {
        (None, None) => None,
        (Some(ours), None) => Some(ours.clone()),
        (None, Some(theirs)) => Some(theirs.clone()),
        (Some(ours), Some(theirs)) => Some(LockfileSettings {
            auto_install_peers: ours.auto_install_peers || theirs.auto_install_peers,
            dedupe_peers: ours.dedupe_peers.or(theirs.dedupe_peers),
            exclude_links_from_lockfile: ours.exclude_links_from_lockfile
                || theirs.exclude_links_from_lockfile,
            inject_workspace_packages: ours.inject_workspace_packages
                || theirs.inject_workspace_packages,
            peers_suffix_max_length: ours.peers_suffix_max_length
                .or(theirs.peers_suffix_max_length),
        }),
    }
}

pub(super) fn merge_catalogs(
    ours: Option<&CatalogSnapshots>,
    theirs: Option<&CatalogSnapshots>,
) -> Option<CatalogSnapshots> {
    if ours.is_none() && theirs.is_none() {
        return None;
    }
    let mut merged: CatalogSnapshots = ours.cloned().unwrap_or_default();
    for (catalog_name, their_entries) in theirs.cloned().unwrap_or_default() {
        let our_entries = merged.entry(catalog_name).or_default();
        for (dep_name, their_entry) in their_entries {
            let entry = match our_entries.get(&dep_name) {
                Some(our_entry) => merge_catalog_entry(our_entry, &their_entry),
                None => their_entry,
            };
            our_entries.insert(dep_name, entry);
        }
    }
    (!merged.is_empty()).then_some(merged)
}

fn merge_catalog_entry(
    ours: &ResolvedCatalogEntry,
    theirs: &ResolvedCatalogEntry,
) -> ResolvedCatalogEntry {
    ResolvedCatalogEntry {
        specifier: take_changed(&ours.specifier, &theirs.specifier),
        version: match winner(&ours.version, &theirs.version) {
            Winner::Ours => ours.version.clone(),
            Winner::Theirs => theirs.version.clone(),
        },
    }
}

pub(super) fn merge_overrides(
    ours: Option<&IndexMap<String, String>>,
    theirs: Option<&IndexMap<String, String>>,
) -> Option<IndexMap<String, String>> {
    match (ours, theirs) {
        (None, None) => None,
        (Some(ours), None) => (!ours.is_empty()).then(|| ours.clone()),
        (None, Some(theirs)) => (!theirs.is_empty()).then(|| theirs.clone()),
        (Some(ours), Some(theirs)) => {
            let mut merged = ours.clone();
            for (key, value) in theirs {
                merged.insert(key.clone(), value.clone());
            }
            (!merged.is_empty()).then_some(merged)
        }
    }
}

pub(super) fn merge_patched_dependencies(
    ours: Option<&BTreeMap<String, String>>,
    theirs: Option<&BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    match (ours, theirs) {
        (None, None) => None,
        (Some(ours), None) => (!ours.is_empty()).then(|| ours.clone()),
        (None, Some(theirs)) => (!theirs.is_empty()).then(|| theirs.clone()),
        (Some(ours), Some(theirs)) => {
            let mut merged = ours.clone();
            for (key, value) in theirs {
                merged.insert(key.clone(), value.clone());
            }
            (!merged.is_empty()).then_some(merged)
        }
    }
}

pub(super) fn merge_time(
    ours: Option<&BTreeMap<String, String>>,
    theirs: Option<&BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    match (ours, theirs) {
        (None, None) => None,
        (Some(ours), None) => (!ours.is_empty()).then(|| ours.clone()),
        (None, Some(theirs)) => (!theirs.is_empty()).then(|| theirs.clone()),
        (Some(ours), Some(theirs)) => {
            let mut merged = ours.clone();
            for (key, value) in theirs {
                merged.insert(key.clone(), value.clone());
            }
            (!merged.is_empty()).then_some(merged)
        }
    }
}
