use super::importer_root_dir;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Partition validated importer keys into the concurrency-safe task
/// groups the parallel link pass runs.
///
/// Distinct keys may alias one directory through the filesystem's own
/// name folding — case-insensitivity, Unicode normalization (APFS),
/// Windows short names and trailing dots — and a real pnpm lockfile
/// can't produce them (project discovery would have collapsed the
/// directories), but a hostile lockfile can, and two tasks mutating
/// one `node_modules` would race. Rather than enumerate the folding
/// rules, ask the filesystem: importers whose project dirs
/// canonicalize to one path share a group, in the caller's (sorted)
/// order, keeping the serial pass's deterministic last-writer outcome,
/// while distinct projects pay one read-only `canonicalize` each. A
/// project dir that doesn't exist yet has nothing on disk to
/// canonicalize against — and no string transform can decide which
/// not-yet-created names the filesystem will later fold together — so
/// every canonicalization failure lands in one shared serial group.
/// That costs nothing real: a genuine project's directory always
/// exists by this point (its manifest was read during project
/// discovery), so the shared group only ever collects the phantom
/// importers of a malformed lockfile.
pub(super) fn importer_task_groups<'a>(
    workspace_root: &Path,
    keys: Vec<&'a str>,
) -> Vec<ImporterTaskGroup<'a>> {
    let mut task_groups: BTreeMap<PathBuf, Vec<&'a str>> = BTreeMap::new();
    let mut unresolved: Vec<&'a str> = Vec::new();
    for importer_id in keys {
        let project_dir = importer_root_dir(workspace_root, importer_id);
        match std::fs::canonicalize(&project_dir) {
            Ok(canonical) => task_groups
                .entry(canonical)
                .or_default()
                .push(importer_id),
            Err(_) => unresolved.push(importer_id),
        }
    }
    let mut task_groups: Vec<ImporterTaskGroup<'a>> = task_groups
        .into_iter()
        .map(|(real_dir, importer_ids)| ImporterTaskGroup {
            real_dir: Some(real_dir),
            importer_ids,
        })
        .collect();
    if !unresolved.is_empty() {
        task_groups.push(ImporterTaskGroup { real_dir: None, importer_ids: unresolved });
    }
    task_groups
}

/// Importers the parallel link pass runs serially in one task, and the
/// directory they canonicalize to (`None` for the group of project dirs
/// that do not exist).
pub(super) struct ImporterTaskGroup<'a> {
    pub(super) real_dir: Option<PathBuf>,
    pub(super) importer_ids: Vec<&'a str>,
}

/// The directory an importer's modules dir is created in.
///
/// The OS resolves a relative symlink from the real location of the
/// directory holding it, so the links of a project reached through a
/// symlink under the workspace root (`packages/foo -> ../../elsewhere/foo`)
/// are computed from its real directory. Every other project keeps its
/// lexical path, leaving the link contents of an ordinary workspace
/// unchanged.
pub(super) fn importer_modules_parent<'a>(
    workspace_root_real: Option<&Path>,
    project_dir: &'a Path,
    real_dir: Option<&'a Path>,
    importer_id: &str,
) -> &'a Path {
    let (Some(workspace_root_real), Some(real_dir)) = (workspace_root_real, real_dir) else {
        return project_dir;
    };
    if importer_id == "." {
        return project_dir;
    }
    let lexical_real = importer_id
        .split('/')
        .fold(workspace_root_real.to_path_buf(), |dir, segment| dir.join(segment));
    if real_dir == lexical_real { project_dir } else { real_dir }
}
