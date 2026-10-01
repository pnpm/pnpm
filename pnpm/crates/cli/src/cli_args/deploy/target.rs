use super::{
    AtomicU8, Context, DeployError, DeployFiles, DirectoryFetcher, ImportIndexedDirOpts,
    IntoDiagnostic, Lockfile, PackageImportMethod, PackageManifest, Path, PathBuf, Reporter, Value,
    WORKSPACE_MANIFEST_FILENAME, apply_deploy_manifest_hook, fs, import_indexed_dir, io,
    is_ancestor_path, is_child_path, lexical_normalize, path_compare::has_path_prefix,
    remove_dirent, same_path, warn,
};
use pnpm_local_spec::LocalSpec;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

pub(super) fn resolve_target_dir(dir: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        lexical_normalize(target)
    } else {
        lexical_normalize(&dir.join(target))
    }
}

/// The prepared deploy directory, as the deploy files diff their paths from
/// and the deploy install resolves them against.
///
/// Those files record paths relative to the deploy directory. On Unix the
/// kernel resolves a `..` from the real directory, so paths diffed from a
/// target under a symlink, such as macOS `/tmp`, would climb out of the
/// symlink's target. Windows collapses `..` lexically, so the target the
/// deploy was given resolves correctly there.
#[cfg(unix)]
pub(super) fn real_deploy_dir(deploy_dir: &Path) -> miette::Result<PathBuf> {
    fs::canonicalize(deploy_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve deploy directory {}", deploy_dir.display()))
}

#[cfg(not(unix))]
pub(super) fn real_deploy_dir(deploy_dir: &Path) -> miette::Result<PathBuf> {
    Ok(deploy_dir.to_path_buf())
}

pub(super) fn validate_deploy_target(
    deploy_dir: &Path,
    workspace_dir: &Path,
    project_dir: &Path,
    dir: &Path,
    force: bool,
) -> miette::Result<()> {
    let deploy_dir = lexical_normalize(deploy_dir);
    let workspace_dir = lexical_normalize(workspace_dir);
    let project_dir = lexical_normalize(project_dir);
    let dir = lexical_normalize(dir);

    if same_path(&deploy_dir, &workspace_dir) {
        return unsafe_deploy_target(&deploy_dir, "target is the workspace root");
    }
    if is_ancestor_path(&deploy_dir, &workspace_dir) {
        return unsafe_deploy_target(&deploy_dir, "target contains the workspace root");
    }
    if same_path(&deploy_dir, &project_dir) {
        return unsafe_deploy_target(&deploy_dir, "target is the selected project root");
    }
    if is_ancestor_path(&deploy_dir, &project_dir) {
        return unsafe_deploy_target(&deploy_dir, "target contains the selected project");
    }
    if same_path(&deploy_dir, &dir) {
        return unsafe_deploy_target(&deploy_dir, "target is the current directory");
    }
    if is_ancestor_path(&deploy_dir, &dir) {
        return unsafe_deploy_target(&deploy_dir, "target contains the current directory");
    }
    if force && !is_child_path(&deploy_dir, &workspace_dir) {
        return unsafe_deploy_target(&deploy_dir, "target is outside the workspace");
    }
    if is_child_path(&deploy_dir, &workspace_dir) {
        validate_workspace_child_target_components(&workspace_dir, &deploy_dir)?;
    }

    Ok(())
}

fn validate_workspace_child_target_components(
    workspace_dir: &Path,
    deploy_dir: &Path,
) -> miette::Result<()> {
    let mut current = workspace_dir.to_path_buf();
    for component in relative_components_from_child(workspace_dir, deploy_dir)? {
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("inspect deploy target {}", current.display()));
            }
        };
        if is_unsafe_deploy_link(&metadata) {
            return unsafe_deploy_target(&current, "target path contains a symlink or junction");
        }
    }
    Ok(())
}

fn unsafe_deploy_target<Output>(deploy_dir: &Path, reason: &'static str) -> miette::Result<Output> {
    Err(DeployError::UnsafeDeployTarget { deploy_dir: deploy_dir.to_path_buf(), reason }.into())
}

pub(super) fn prepare_deploy_dir<ReporterT: Reporter>(
    workspace_dir: &Path,
    deploy_dir: &Path,
    force: bool,
) -> miette::Result<()> {
    let workspace_dir = lexical_normalize(workspace_dir);
    let deploy_dir = lexical_normalize(deploy_dir);
    let workspace_child_target = is_child_path(&deploy_dir, &workspace_dir);
    if workspace_child_target {
        create_workspace_child_target_parents(&workspace_dir, &deploy_dir)?;
    }
    if !is_empty_dir_or_absent(&deploy_dir)? {
        if !force {
            return Err(DeployError::DeployDirNotEmpty { deploy_dir }.into());
        }
        warn::<ReporterT>(
            &deploy_dir,
            format!("using --force, deleting deploy path {}", deploy_dir.display()),
        );
    }
    if workspace_child_target {
        validate_workspace_child_target_components(&workspace_dir, &deploy_dir)?;
    }
    remove_path_if_exists(&deploy_dir)?;
    if workspace_child_target {
        create_workspace_child_target_parents(&workspace_dir, &deploy_dir)?;
        create_workspace_child_target_dir(&workspace_dir, &deploy_dir)
    } else {
        fs::create_dir_all(&deploy_dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("create deploy directory {}", deploy_dir.display()))
    }
}

fn create_workspace_child_target_parents(
    workspace_dir: &Path,
    deploy_dir: &Path,
) -> miette::Result<()> {
    let Some(parent) = deploy_dir.parent() else {
        return Ok(());
    };
    let mut current = workspace_dir.to_path_buf();
    for component in relative_components_from_child(workspace_dir, parent)? {
        current.push(component);
        create_workspace_child_target_component(&current)?;
    }
    Ok(())
}

fn create_workspace_child_target_dir(
    workspace_dir: &Path,
    deploy_dir: &Path,
) -> miette::Result<()> {
    match fs::create_dir(deploy_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return unsafe_deploy_target(deploy_dir, "target changed during deploy preparation");
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("create deploy directory {}", deploy_dir.display()));
        }
    }
    validate_workspace_child_target_components(workspace_dir, deploy_dir)
}

fn create_workspace_child_target_component(component: &Path) -> miette::Result<()> {
    match fs::create_dir(component) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("create deploy target component {}", component.display())
                });
        }
    }
    let metadata = fs::symlink_metadata(component)
        .into_diagnostic()
        .wrap_err_with(|| format!("inspect deploy target {}", component.display()))?;
    if is_unsafe_deploy_link(&metadata) {
        return unsafe_deploy_target(component, "target path contains a symlink or junction");
    }
    if !metadata.is_dir() {
        return unsafe_deploy_target(component, "target path contains a non-directory");
    }
    Ok(())
}

fn is_empty_dir_or_absent(path: &Path) -> miette::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error).into_diagnostic(),
    };
    if !metadata.is_dir() || is_unsafe_deploy_link(&metadata) {
        return Ok(false);
    }
    let mut entries = fs::read_dir(path).into_diagnostic()?;
    Ok(entries.next().is_none())
}

fn remove_path_if_exists(path: &Path) -> miette::Result<()> {
    match remove_dirent(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| format!("remove deploy path {}", path.display())),
    }
}

fn is_unsafe_deploy_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(super) fn copy_project(
    src: &Path,
    dest: &Path,
    include_only_package_files: bool,
) -> miette::Result<()> {
    let output = DirectoryFetcher {
        directory: src.to_path_buf(),
        include_only_package_files,
        resolve_symlinks: false,
        preserve_symlinks: true,
        allow_path_escape: false,
    }
    .run()
    .map_err(miette::Report::new)
    .wrap_err("fetch project files")?;
    let logged_methods = AtomicU8::new(0);
    import_indexed_dir::<pnpm_reporter::SilentReporter>(
        &logged_methods,
        PackageImportMethod::CloneOrCopy,
        dest,
        &output.files_map,
        ImportIndexedDirOpts {
            force: true,
            preserve_symlinks: true,
            ..ImportIndexedDirOpts::default()
        },
    )
    .map_err(miette::Report::new)
    .wrap_err("copy project files")
}

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

fn relative_components_from_child(parent: &Path, child: &Path) -> miette::Result<Vec<PathBuf>> {
    let parent = lexical_normalize(parent);
    let child = lexical_normalize(child);
    if !has_path_prefix(&child, &parent) {
        child.strip_prefix(&parent).into_diagnostic()?;
    }
    Ok(child
        .components()
        .skip(parent.components().count())
        .map(|component| PathBuf::from(component.as_os_str()))
        .collect())
}

pub(super) fn relative_path(from: &Path, to: &Path) -> String {
    let relative = pathdiff::diff_paths(to, from).unwrap_or_else(|| to.to_path_buf());
    relative.to_string_lossy().replace('\\', "/")
}

pub(super) fn write_deploy_files(
    deploy_dir: &Path,
    deploy_files: &DeployFiles,
) -> miette::Result<()> {
    let mut manifest = serde_json::to_string_pretty(&deploy_files.manifest).into_diagnostic()?;
    manifest.push('\n');
    let lockfile = deploy_files.lockfile
        .to_yaml_string()
        .map_err(miette::Report::new)
        .wrap_err("serialize deployed lockfile")?;
    write_atomic(&deploy_dir.join(Lockfile::FILE_NAME), lockfile.as_bytes())
        .into_diagnostic()
        .wrap_err("write deployed lockfile")?;
    if let Some(workspace_manifest) = &deploy_files.workspace_manifest {
        let workspace_manifest = serde_saphyr::to_string(workspace_manifest)
            .into_diagnostic()
            .wrap_err("serialize deployed workspace manifest")?;
        write_atomic(&deploy_dir.join(WORKSPACE_MANIFEST_FILENAME), workspace_manifest.as_bytes())
            .into_diagnostic()
            .wrap_err("write deployed workspace manifest")?;
    }
    write_atomic(&deploy_dir.join("package.json"), manifest.as_bytes())
        .into_diagnostic()
        .wrap_err("write deployed package.json")?;
    Ok(())
}

mod atomic_write;
use atomic_write::write_atomic;
