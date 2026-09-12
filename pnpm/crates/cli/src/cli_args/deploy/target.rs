use super::{
    AtomicU8, Context, DeployError, DeployFiles, DirectoryFetcher, ImportIndexedDirOpts,
    IntoDiagnostic, Lockfile, PackageImportMethod, PackageManifest, Path, PathBuf, Reporter, Value,
    WORKSPACE_MANIFEST_FILENAME, Write, apply_deploy_manifest_hook, fs, import_indexed_dir, io,
    lexical_normalize, remove_dirent, warn,
};
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

pub(super) fn is_ancestor_path(parent: &Path, child: &Path) -> bool {
    is_child_path(child, parent)
}

pub(super) fn is_child_path(child: &Path, parent: &Path) -> bool {
    has_path_prefix(child, parent) && !same_path(child, parent)
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
            return Err(error).into_diagnostic().wrap_err_with(|| {
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

pub(super) fn copy_project<ReporterT: Reporter>(
    src: &Path,
    dest: &Path,
    include_only_package_files: bool,
) -> miette::Result<()> {
    let output = DirectoryFetcher {
        directory: src.to_path_buf(),
        include_only_package_files,
        resolve_symlinks: false,
        allow_path_escape: false,
    }
    .run()
    .map_err(miette::Report::new)
    .wrap_err("fetch project files")?;
    let logged_methods = AtomicU8::new(0);
    import_indexed_dir::<ReporterT>(
        &logged_methods,
        PackageImportMethod::CloneOrCopy,
        dest,
        &output.files_map,
        ImportIndexedDirOpts { force: true, ..ImportIndexedDirOpts::default() },
    )
    .map_err(miette::Report::new)
    .wrap_err("copy project files")
}

pub(super) fn apply_deploy_hook(manifest_path: &Path) -> miette::Result<()> {
    let mut manifest = PackageManifest::from_path(manifest_path.to_path_buf())
        .wrap_err("read deployed manifest")?;
    apply_deploy_manifest_hook(manifest.value_mut());
    manifest.save().wrap_err("write deployed manifest")
}

pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    let left = lexical_normalize(left);
    let right = lexical_normalize(right);
    path_components_match(&left, &right)
}

/// Hash key under which two paths collide exactly when [`same_path`] equates
/// them.
#[derive(PartialEq, Eq, Hash)]
pub(super) struct ProjectPathKey(Vec<String>);

impl ProjectPathKey {
    pub(super) fn new(path: &Path) -> Self {
        Self(comparable_path_components(&lexical_normalize(path)))
    }
}

fn has_path_prefix(child: &Path, parent: &Path) -> bool {
    let child = lexical_normalize(child);
    let parent = lexical_normalize(parent);
    let child_components = comparable_path_components(&child);
    let parent_components = comparable_path_components(&parent);
    child_components.len() >= parent_components.len()
        && child_components
            .iter()
            .zip(parent_components.iter())
            .all(|(child, parent)| child == parent)
}

fn path_components_match(left: &Path, right: &Path) -> bool {
    comparable_path_components(left) == comparable_path_components(right)
}

fn comparable_path_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| comparison_component(component.as_os_str().to_string_lossy().as_ref()))
        .collect()
}

#[cfg(windows)]
fn comparison_component(component: &str) -> String {
    component.to_lowercase()
}

#[cfg(not(windows))]
fn comparison_component(component: &str) -> String {
    component.to_string()
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
    let lockfile = deploy_files
        .lockfile
        .to_yaml_string()
        .map_err(miette::Report::new)
        .wrap_err("serialize deployed lockfile")?;
    write_atomic(&deploy_dir.join(Lockfile::FILE_NAME), lockfile.as_bytes())
        .into_diagnostic()
        .wrap_err("write deployed lockfile")?;
    if let Some(workspace_manifest) = &deploy_files.workspace_manifest {
        write_atomic(
            &deploy_dir.join(WORKSPACE_MANIFEST_FILENAME),
            workspace_manifest_yaml(workspace_manifest).as_bytes(),
        )
        .into_diagnostic()
        .wrap_err("write deployed workspace manifest")?;
    }
    write_atomic(&deploy_dir.join("package.json"), manifest.as_bytes())
        .into_diagnostic()
        .wrap_err("write deployed package.json")?;
    Ok(())
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    if let Ok(metadata) = fs::metadata(path) {
        tmp.as_file().set_permissions(metadata.permissions())?;
    }
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn workspace_manifest_yaml(workspace_manifest: &Value) -> String {
    let mut out = String::new();
    let Some(object) = workspace_manifest.as_object() else { return out };
    for field in ["patchedDependencies", "allowBuilds"] {
        let Some(values) = object.get(field).and_then(Value::as_object) else { continue };
        out.push_str(field);
        out.push_str(":\n");
        for (key, value) in values {
            out.push_str("  ");
            out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| format!("{key:?}")));
            out.push_str(": ");
            out.push_str(&serde_json::to_string(value).unwrap_or_else(|_| value.to_string()));
            out.push('\n');
        }
    }
    out
}
