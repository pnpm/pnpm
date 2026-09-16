//! Which projects in this repository a project reaches through the sources
//! it declares: the edges a `--filter` selector follows, and the set of
//! projects an install ends up reading.

use super::{
    super::manifest::{Manifest, Source, SourceDeclaration},
    Workspace,
};
use pep508_rs::PackageName;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

impl Workspace {
    /// Every project whose manifest one of `roots` reads: the selected
    /// projects themselves, and the ones they reach through declared
    /// sources. A project outside this set takes no part in the install, so
    /// nothing needs its metadata prepared.
    pub(in super::super) fn reachable_from(&self, roots: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
        let mut reachable = BTreeSet::new();
        let mut frontier = roots
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        while let Some(root) = frontier.pop() {
            let Some(manifest) = self.manifests.get(&root) else { continue };
            if !reachable.insert(root.clone()) {
                continue;
            }
            frontier.extend(self.declared_sources(&root, manifest));
        }
        reachable
    }

    /// The projects in this repository the project at `root` declares a
    /// source for.
    ///
    /// These are the edges a `--filter` selector follows when it asks for a
    /// project's dependencies or dependents. They are read from the
    /// declarations rather than from the requirements that consult them, so
    /// a project whose requirements a build backend generates is covered
    /// too.
    pub(in super::super) fn declared_sources(
        &self,
        root: &Path,
        manifest: &Manifest,
    ) -> Vec<PathBuf> {
        let mut targets = Vec::new();
        for (declared_by, sources) in self.source_tables(root, manifest) {
            for (name, declaration) in sources {
                self.collect_targets(root, declared_by, (name, declaration), &mut targets);
            }
        }
        targets
    }

    /// The `[tool.uv.sources]` tables the project at `root` resolves
    /// through: its own, and the one its workspace root declares for its
    /// members.
    fn source_tables<'a>(
        &'a self,
        root: &'a Path,
        manifest: &'a Manifest,
    ) -> Vec<(&'a Path, &'a BTreeMap<PackageName, SourceDeclaration>)> {
        let mut tables = vec![(root, &manifest.tool.uv.sources)];
        if let Some((declared_in, inherited)) = self.inherited.get(root) {
            tables.push((declared_in.as_path(), &inherited.tool.uv.sources));
        }
        tables
    }

    /// Add the discovered projects one declaration points at, leaving out
    /// the declaring project itself and anything pnpm did not discover.
    fn collect_targets(
        &self,
        root: &Path,
        declared_by: &Path,
        (name, declaration): (&PackageName, &SourceDeclaration),
        targets: &mut Vec<PathBuf>,
    ) {
        for source in declaration.sources() {
            let Some(target) = self.source_target(declared_by, name, source) else { continue };
            if target != root && self.manifests.contains_key(&target) && !targets.contains(&target)
            {
                targets.push(target);
            }
        }
    }

    /// Where one source points, for a path or workspace source. A source
    /// pnpm resolves from somewhere other than this repository points at no
    /// project in it.
    fn source_target(
        &self,
        declared_by: &Path,
        name: &PackageName,
        source: &Source,
    ) -> Option<PathBuf> {
        if let Some(path) = &source.path {
            return Some(pnpm_fs::lexical_normalize(&declared_by.join(path)));
        }
        if !source.workspace {
            return None;
        }
        self.roots.get(name)?.first().cloned()
    }
}
