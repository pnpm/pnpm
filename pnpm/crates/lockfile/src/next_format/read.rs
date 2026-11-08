//! The next format back to peer-suffixed sections.
//!
//! 1. Every reference is matched to one variant of the package it names. A
//!    package with one variant needs no match. A reference that carries peer
//!    groups names its variant's providers. Any other reference is matched to
//!    the variant whose bindings all agree with the referrer's scope, binding
//!    the most names.
//! 2. Every peer binding is matched to its provider: the variant of the
//!    provider that the scope above the binding snapshot links under the
//!    peer's name. The scope of a variant is that of the referrer that first
//!    reaches it, in a walk from the importers in order.
//! 3. Each variant's key is rebuilt from its providers, with the rules the
//!    resolver uses for the peer suffix: a provider is written as
//!    `name@version` under `dedupePeers`, for a link, and inside a peer cycle,
//!    and as its own key otherwise.
//!
//! The writer checks that this reads its output back to the sections it was
//! given, and writes a reference with its peer groups wherever step 1 or 2
//! would pick another variant.

mod choose;
mod entries;
mod keys;
mod scope;

use super::{
    NextProjectSnapshot, NextSections, NextSnapshotEntry, NextSnapshots, RestoreContext,
    RestoredSections,
};
use crate::{PackageKey, PkgName, PkgNameVerPeer, PkgVerPeer};
use rayon::prelude::*;
use std::collections::HashMap;

/// Rebuild the peer-suffixed `importers` and `snapshots` from the next
/// format's.
#[must_use]
pub fn restore_sections(sections: &NextSections, context: RestoreContext<'_>) -> RestoredSections {
    restore_with_variant_keys(sections, context).sections
}

/// [`restore_sections`], plus the key each variant was given, in the order of
/// [`Graph::index`].
pub(super) struct Restored {
    pub sections: RestoredSections,
    pub variant_keys: HashMap<PackageKey, Vec<PkgNameVerPeer>>,
}

pub(super) fn restore_with_variant_keys(
    sections: &NextSections,
    context: RestoreContext<'_>,
) -> Restored {
    let Some(snapshots) = sections.snapshots.as_ref() else {
        let importers = sections.importers
            .iter()
            .map(|(id, importer)| (id.clone(), importer.snapshot.clone()))
            .collect();
        let sections = RestoredSections { importers, snapshots: None };
        return Restored { sections, variant_keys: HashMap::new() };
    };
    let graph = Graph::new(snapshots, &sections.importers);
    let keys = graph.keys(context);
    let snapshots = graph.snapshot_entries(&keys, context);
    let importers = sections.importers
        .par_iter()
        .map(|(id, importer)| (id.clone(), graph.restore_importer(id, importer, &keys)))
        .collect();
    let variant_keys = graph.variants
        .iter()
        .map(|(key, &(start, count))| ((*key).clone(), keys[start..start + count].to_vec()))
        .collect();
    Restored { sections: RestoredSections { importers, snapshots: Some(snapshots) }, variant_keys }
}

type Vid = usize;

#[derive(Clone, Copy)]
enum Referrer<'a> {
    Importer(&'a str),
    Snapshot(Vid),
}

enum Provider {
    Variant(Vid),
    Link(String),
    /// The lockfile has no snapshot of the provider.
    Missing(String),
}

struct Graph<'a> {
    keys: Vec<&'a PackageKey>,
    entries: Vec<&'a NextSnapshotEntry>,
    variants: HashMap<&'a PackageKey, (Vid, usize)>,
    importers: HashMap<&'a str, &'a NextProjectSnapshot>,
    /// Each variant's chosen children, sorted by name.
    children: Vec<Vec<(&'a PkgName, Vid)>>,
    importer_children: HashMap<&'a str, Vec<(&'a PkgName, Vid)>>,
    /// The referrer that first reaches each variant: the scope its peers are
    /// bound in.
    parent: Vec<Option<Referrer<'a>>>,
    providers: Vec<Vec<(&'a PkgName, Provider)>>,
}

impl<'a> Graph<'a> {
    fn new(
        snapshots: &'a NextSnapshots,
        importers: &'a HashMap<String, NextProjectSnapshot>,
    ) -> Self {
        let mut graph = Graph::index(snapshots, importers);
        graph.children = (0..graph.keys.len())
            .into_par_iter()
            .map(|vid| graph.choose_children(vid))
            .collect();
        graph.importer_children = graph.importers
            .par_iter()
            .map(|(&id, importer)| (id, graph.choose_importer_children(id, importer)))
            .collect();
        graph.parent = graph.parents();
        graph.providers = (0..graph.keys.len())
            .into_par_iter()
            .map(|vid| {
                graph.entries[vid]
                    .bindings()
                    .map(|(name, reference)| (name, graph.provider(vid, name, reference)))
                    .collect()
            })
            .collect();
        graph
    }

    fn index(
        snapshots: &'a NextSnapshots,
        importers: &'a HashMap<String, NextProjectSnapshot>,
    ) -> Self {
        let mut keys = Vec::new();
        let mut entries = Vec::new();
        let mut variants = HashMap::with_capacity(snapshots.len());
        for (key, list) in snapshots {
            variants.insert(key, (keys.len(), list.len()));
            for entry in list {
                keys.push(key);
                entries.push(entry);
            }
        }
        Graph {
            keys,
            entries,
            variants,
            importers: importers
                .iter()
                .map(|(id, importer)| (id.as_str(), importer))
                .collect(),
            children: Vec::new(),
            importer_children: HashMap::new(),
            parent: Vec::new(),
            providers: Vec::new(),
        }
    }
}

/// An order of package names that every process agrees on.
fn compare_names(left: &PkgName, right: &PkgName) -> std::cmp::Ordering {
    (&left.scope, &left.bare).cmp(&(&right.scope, &right.bare))
}

fn child_named(children: &[(&PkgName, Vid)], name: &PkgName) -> Option<Vid> {
    children
        .binary_search_by(|(alias, _)| compare_names(alias, name))
        .ok()
        .map(|index| children[index].1)
}

/// Whether the reference carries peer groups, not only a patch group.
fn has_peer_groups(version: &PkgVerPeer) -> bool {
    version.peer().len() > version.patch_group().len()
}
