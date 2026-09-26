use super::{BTreeSet, ManifestFormat, Path, PathBuf};

/// Group the manifests by the root directory they belong to, in `rootDir`
/// order.
///
/// A root's candidates are in `preferred`'s manifest-precedence order and
/// share one read task, so "first readable manifest wins" holds under
/// concurrency: a candidate that vanishes mid-run hands its root to the next
/// candidate, never to a skipped root.
pub(super) fn group_manifests_by_root(
    manifest_paths: BTreeSet<PathBuf>,
    workspace_root: &Path,
    preferred: ManifestFormat,
) -> Vec<(PathBuf, Vec<PathBuf>)> {
    let mut sorted: Vec<PathBuf> = manifest_paths.into_iter().collect();
    sorted.sort_by(|left, right| {
        manifest_order_key(left, preferred).cmp(&manifest_order_key(right, preferred))
    });
    let mut root_groups: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for manifest_path in sorted {
        let root_dir = manifest_path
            .parent()
            .unwrap_or(workspace_root)
            .to_path_buf();
        match root_groups.last_mut() {
            Some((last_root, candidates)) if *last_root == root_dir => {
                candidates.push(manifest_path);
            }
            _ => root_groups.push((root_dir, vec![manifest_path])),
        }
    }
    root_groups
}

/// Sort key placing a manifest by its directory, then by its basename's
/// place in `preferred`'s precedence.
fn manifest_order_key(path: &Path, preferred: ManifestFormat) -> (&Path, usize) {
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let rank = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| {
            preferred
                .precedence()
                .position(|basename| basename == name)
        })
        .unwrap_or(usize::MAX);
    (dir, rank)
}
