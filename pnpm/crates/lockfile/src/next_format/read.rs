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

use super::{NextSections, NextSnapshotEntry, NextSnapshots, RestoreContext, RestoredSections};
use crate::{PackageKey, PkgName, PkgNameVerPeer, PkgVerPeer, ProjectSnapshot};
use rayon::prelude::*;
use std::collections::HashMap;

/// Rebuild the peer-suffixed `importers` and `snapshots` from the next
/// format's.
#[must_use]
pub fn restore_sections(sections: &NextSections, context: RestoreContext<'_>) -> RestoredSections {
    match Rebuilt::of(sections, context) {
        Some(rebuilt) => rebuilt.into_sections(sections, context),
        None => RestoredSections { importers: sections.importers.clone(), snapshots: None },
    }
}

/// The reference graph of a next-format document, with every variant's
/// peer-suffixed key: what the writer checks its output against, and what
/// the peer-suffixed sections are built from.
pub(super) struct Rebuilt<'a> {
    graph: Graph<'a>,
    keys: Vec<PkgNameVerPeer>,
}

impl<'a> Rebuilt<'a> {
    /// `None` when the document has no `snapshots`.
    pub(super) fn of(sections: &'a NextSections, context: RestoreContext<'_>) -> Option<Self> {
        let snapshots = sections.snapshots.as_ref()?;
        let graph = Graph::new(snapshots, &sections.importers);
        let keys = graph.keys(context);
        Some(Rebuilt { graph, keys })
    }

    fn vid(&self, package: &PackageKey, index: usize) -> Option<Vid> {
        let &(start, count) = self.graph.variants.get(package)?;
        (index < count).then_some(start + index)
    }

    /// The peer-suffixed key of the variant at `index` under `package`.
    pub(super) fn variant_key(
        &self,
        package: &PackageKey,
        index: usize,
    ) -> Option<&PkgNameVerPeer> {
        self.vid(package, index)
            .map(|vid| &self.keys[vid])
    }

    /// The peer-suffixed key of the child linked as `alias` by that variant.
    pub(super) fn child_key(
        &self,
        package: &PackageKey,
        index: usize,
        alias: &PkgName,
    ) -> Option<&PkgNameVerPeer> {
        let vid = self.vid(package, index)?;
        child_named(&self.graph.children[vid], alias).map(|child| &self.keys[child])
    }

    /// The peer-suffixed key of the child linked as `alias` by an importer.
    pub(super) fn importer_child_key(&self, id: &str, alias: &PkgName) -> Option<&PkgNameVerPeer> {
        let children = self.graph.importer_children.get(id)?;
        child_named(children, alias).map(|child| &self.keys[child])
    }

    /// The peer-suffixed sections.
    pub(super) fn into_sections(
        self,
        sections: &NextSections,
        context: RestoreContext<'_>,
    ) -> RestoredSections {
        let snapshots = self.graph.snapshot_entries(&self.keys, context);
        let importers = sections.importers
            .par_iter()
            .map(|(id, importer)| {
                (id.clone(), self.graph.restore_importer(id, importer, &self.keys))
            })
            .collect();
        RestoredSections { importers, snapshots: Some(snapshots) }
    }
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
    importers: HashMap<&'a str, &'a ProjectSnapshot>,
    /// Each variant's chosen children, sorted by name.
    children: Vec<Vec<(&'a PkgName, Vid)>>,
    importer_children: HashMap<&'a str, Vec<(&'a PkgName, Vid)>>,
    /// The referrer that first reaches each variant: the scope its peers are
    /// bound in.
    parent: Vec<Option<Referrer<'a>>>,
    providers: Vec<Vec<(&'a PkgName, Provider)>>,
}

impl<'a> Graph<'a> {
    fn new(snapshots: &'a NextSnapshots, importers: &'a HashMap<String, ProjectSnapshot>) -> Self {
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
        importers: &'a HashMap<String, ProjectSnapshot>,
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
