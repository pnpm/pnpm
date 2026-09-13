use super::Path;

/// Convert a workspace directory resolution into the representation shared by
/// every importer. A `link:` target is stored relative to the lockfile root;
/// an injected `file:` target already has that representation.
pub(in super::super) fn canonical_workspace_resolution(
    result: &pnpm_resolving_resolver_base::ResolveResult,
    project_dir: &Path,
    lockfile_dir: &Path,
) -> Option<pnpm_resolving_resolver_base::ResolveResult> {
    if result.resolved_via != "workspace" {
        return None;
    }
    let pnpm_lockfile::LockfileResolution::Directory(directory_resolution) = &result.resolution
    else {
        return None;
    };
    if result.id.as_str().strip_prefix("file:") == Some(&directory_resolution.directory) {
        return Some(result.clone());
    }
    if result.id.as_str().strip_prefix("link:") != Some(&directory_resolution.directory) {
        return None;
    }

    let canonical_target =
        canonical_workspace_target(&directory_resolution.directory, project_dir, lockfile_dir);
    let mut canonical = result.clone();
    canonical.id =
        pnpm_resolving_resolver_base::PkgResolutionId::from(format!("link:{canonical_target}"));
    let pnpm_lockfile::LockfileResolution::Directory(canonical_directory) =
        &mut canonical.resolution
    else {
        unreachable!("the cloned workspace resolution remains a directory")
    };
    canonical_directory.directory = canonical_target;
    Some(canonical)
}

/// Render a canonical workspace resolution for one consuming importer before
/// its manifest hooks run.
pub(in super::super) fn render_workspace_resolution(
    canonical: &pnpm_resolving_resolver_base::ResolveResult,
    anchor: &crate::link_target::ImporterAnchor,
    project_dir: &Path,
    lockfile_dir: &Path,
) -> pnpm_resolving_resolver_base::ResolveResult {
    let mut rendered = canonical.clone();
    let pnpm_lockfile::LockfileResolution::Directory(directory_resolution) =
        &mut rendered.resolution
    else {
        unreachable!("the shared workspace cache contains only directory resolutions")
    };
    if canonical.id.as_str().starts_with("file:") {
        return rendered;
    }

    let target = directory_resolution.directory.as_str();
    let consumer_target = anchor
        .target_relative_to_importer(target)
        .unwrap_or_else(|| {
            let target = Path::new(target);
            let absolute_target = if target.is_absolute() {
                pnpm_fs::lexical_normalize(target)
            } else {
                pnpm_fs::lexical_normalize(&lockfile_dir.join(target))
            };
            let project_dir = pnpm_fs::lexical_normalize(project_dir);
            pathdiff::diff_paths(&absolute_target, project_dir)
                .unwrap_or(absolute_target)
                .display()
                .to_string()
                .replace('\\', "/")
        });
    rendered.id =
        pnpm_resolving_resolver_base::PkgResolutionId::from(format!("link:{consumer_target}"));
    directory_resolution.directory = consumer_target;
    rendered
}

fn canonical_workspace_target(directory: &str, project_dir: &Path, lockfile_dir: &Path) -> String {
    let target = Path::new(directory);
    let absolute_target = if target.is_absolute() {
        pnpm_fs::lexical_normalize(target)
    } else {
        pnpm_fs::lexical_normalize(&project_dir.join(target))
    };
    pathdiff::diff_paths(&absolute_target, lockfile_dir)
        .unwrap_or(absolute_target)
        .display()
        .to_string()
        .replace('\\', "/")
}
