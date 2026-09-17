//! Which projects in this repository a project reaches through the sources
//! it declares: the edges a `--filter` selector follows, and the set of
//! projects an install ends up reading.

use super::{
    super::manifest::{Manifest, Source, SourceDeclaration},
    Workspace,
};
use pep508_rs::PackageName;
use std::{
    collections::BTreeSet,
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
        let mut targets = BTreeSet::new();
        for (declared_by, name, declaration) in self.source_entries(root, manifest) {
            self.collect_targets(root, declared_by, (name, declaration), &mut targets);
        }
        targets.into_iter().collect()
    }

    /// The `[tool.uv.sources]` entries the project at `root` resolves
    /// through: its own, and the ones its workspace root declares for the
    /// names it requires.
    ///
    /// A member reads its root's whole table, so taking all of it would
    /// give every member an edge to every project the workspace declares a
    /// source for. A project whose requirements a build backend generates
    /// names none of them here, and keeps the whole table.
    fn source_entries<'a>(
        &'a self,
        root: &'a Path,
        manifest: &'a Manifest,
    ) -> Vec<(&'a Path, &'a PackageName, &'a SourceDeclaration)> {
        let mut entries = manifest.tool.uv.sources
            .iter()
            .map(|(name, declaration)| (root, name, declaration))
            .collect::<Vec<_>>();
        let Some((declared_in, inherited)) = self.inherited.get(root) else { return entries };
        let required = manifest.declared_requirement_names();
        entries.extend(
            inherited.tool.uv.sources
                .iter()
                .filter(|(name, _)| {
                    required
                        .as_ref()
                        .is_none_or(|required| required.contains(*name))
                })
                .map(|(name, declaration)| (declared_in.as_path(), name, declaration)),
        );
        entries
    }

    /// Add the discovered projects one declaration points at, leaving out
    /// the declaring project itself and anything pnpm did not discover.
    ///
    /// A member of a workspace reads the root's whole source table, so the
    /// set the targets collect into is what keeps a workspace of many
    /// members from comparing every target against every other one.
    fn collect_targets(
        &self,
        root: &Path,
        declared_by: &Path,
        (name, declaration): (&PackageName, &SourceDeclaration),
        targets: &mut BTreeSet<PathBuf>,
    ) {
        for source in declaration.sources() {
            let Some(target) = self.source_target(root, declared_by, name, source) else {
                continue;
            };
            if target != root && self.manifests.contains_key(&target) {
                targets.insert(target);
            }
        }
    }

    /// Where one source points, for a path or workspace source. A source
    /// pnpm resolves from somewhere other than this repository points at no
    /// project in it.
    ///
    /// A workspace source reaches only what the project at `root` may take
    /// from the repository, which is what resolving it will allow. A
    /// workspace two projects declare one distribution in is refused there,
    /// with the context that refusal needs, so it contributes no edge here.
    fn source_target(
        &self,
        root: &Path,
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
        self.member(name, root)
            .ok()
            .flatten()
            .map(Path::to_path_buf)
    }
}
