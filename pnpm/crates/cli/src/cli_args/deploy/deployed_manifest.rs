use super::{
    Context, PackageManifest, Path, PathBuf, Value, apply_deploy_manifest_hook,
    target::{relative_components_from_child, same_path},
};
use pnpm_local_spec::LocalSpec;

/// Prepare the copied manifest for the legacy deploy install, which
/// resolves it from `deploy_dir`. Its local-path specifiers are written
/// relative to `project_dir`, so they are re-anchored to keep naming the
/// same files, except for a path that the deploy copied along with the
/// project: that one keeps naming the copy, so the deploy does not
/// depend on the source checkout. A specifier that already names the
/// copy from `deploy_dir` is left as declared, so a source `readPackage`
/// hook still matches it.
pub(super) fn apply_deploy_hook(deploy_dir: &Path, project_dir: &Path) -> miette::Result<()> {
    let mut manifest = PackageManifest::from_path(deploy_dir.join("package.json"))
        .wrap_err("read deployed manifest")?;
    rebase_local_specifiers(manifest.value_mut(), project_dir, deploy_dir);
    apply_deploy_manifest_hook(manifest.value_mut());
    manifest.save().wrap_err("write deployed manifest")
}

fn rebase_local_specifiers(manifest: &mut Value, project_dir: &Path, deploy_dir: &Path) {
    for field in ["optionalDependencies", "dependencies", "devDependencies", "peerDependencies"] {
        let Some(dependencies) = manifest.get_mut(field).and_then(Value::as_object_mut) else {
            continue;
        };
        for specifier in dependencies.values_mut() {
            if let Some(rebased) = specifier
                .as_str()
                .and_then(|specifier| rebase_local_specifier(specifier, project_dir, deploy_dir))
            {
                *specifier = Value::String(rebased);
            }
        }
    }
}

fn rebase_local_specifier(
    specifier: &str,
    project_dir: &Path,
    deploy_dir: &Path,
) -> Option<String> {
    let local = LocalSpec::parse_filesystem(specifier, project_dir)?;
    let Some(inside_project) =
        copied_path_inside_project(local.absolute_path(), project_dir, deploy_dir)
    else {
        return Some(local.render(Some(deploy_dir)));
    };
    let copy = deploy_dir.join(&inside_project);
    let names_the_copy_as_declared = LocalSpec::parse_filesystem(specifier, deploy_dir)
        .is_some_and(|declared| same_path(declared.absolute_path(), &copy));
    // On Windows the specifier can spell the project directory in another
    // case, and `LocalSpec::render` diffs paths case-sensitively.
    let spelled_project_dir = local
        .absolute_path()
        .ancestors()
        .nth(inside_project.components().count())?;
    (!names_the_copy_as_declared).then(|| local.render(Some(spelled_project_dir)))
}

fn copied_path_inside_project(
    path: &Path,
    project_dir: &Path,
    deploy_dir: &Path,
) -> Option<PathBuf> {
    let inside_project: PathBuf = relative_components_from_child(project_dir, path)
        .ok()?
        .iter()
        .collect();
    deploy_dir
        .join(&inside_project)
        .exists()
        .then_some(inside_project)
}
