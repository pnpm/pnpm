use crate::state::State;
use clap::Args;
use command_extra::CommandExtra;
use miette::{
    Context,
    IntoDiagnostic,
    miette,
};
use pnpm_lockfile::{
    Lockfile,
    PackageKey,
    PeerEdgeOptions,
};
use pnpm_reporter::Reporter;
use std::{
    env,
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
    process::Command,
};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

/// Opens an installed package's folder in the default text editor.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// Name of the package to edit.
    pub package_path: String,

    /// The editor to use for opening the package (overrides config and env).
    #[arg(long)]
    pub editor: Option<String>,
}

impl EditArgs {
    pub async fn run<ReporterType: Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let parts = parse_package_path(&self.package_path)?;
        if state.config.enable_global_virtual_store {
            return Err(miette!(
                code = "ERR_PNPM_EDIT_SHARED_STORE",
                help = "Use pnpm patch to create a project-specific patch",
                "Cannot edit dependencies in a shared virtual store",
            ));
        }
        let real_pkg_path = installed_package_dir(&state, &parts)?;

        let editor = self.pick_editor(&state);
        let project_root =
            state.config.workspace_dir.as_deref().unwrap_or_else(|| state.project_dir());
        let mut command = resolve_editor_command(&editor, project_root, &real_pkg_path)?;
        let package_key = installed_package_key(&state, &parts)?;
        de_hardlink_dir(&real_pkg_path)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to break store hard links for editing in '{}'",
                    real_pkg_path.display(),
                )
            })?;

        let status = command
            .status()
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to execute editor command: {editor}"))?;

        if !status.success() {
            return Err(miette!("Editor command exited with failure status"));
        }

        rebuild_package::<ReporterType>(state, package_key).await?;

        Ok(())
    }

    fn pick_editor(&self, state: &State) -> String {
        self.editor
            .clone()
            .or_else(|| state.config.editor.clone())
            .or_else(|| {
                env::var("EDITOR")
                    .ok()
                    .filter(|editor| !editor.trim().is_empty())
            })
            .or_else(|| {
                env::var("VISUAL")
                    .ok()
                    .filter(|editor| !editor.trim().is_empty())
            })
            .unwrap_or_else(|| if cfg!(windows) { "notepad".to_owned() } else { "vi".to_owned() })
    }
}

fn installed_package_dir(state: &State, parts: &[String]) -> miette::Result<PathBuf> {
    let project_dir = state.project_dir();
    let project_name = state.config
        .applies_package_configs()
        .then(|| pnpm_workspace::read_project_name(project_dir))
        .flatten();
    let project_modules_dir =
        state.config.project_modules_dir(project_dir, project_name.as_deref());
    let workspace_modules_dir = dunce::canonicalize(&state.config.modules_dir).ok();
    resolve_package_dir_with_fallback(
        &project_modules_dir,
        workspace_modules_dir.as_deref(),
        parts,
        state.config.virtual_store_dir(),
    )
}

fn installed_package_key(state: &State, parts: &[String]) -> miette::Result<PackageKey> {
    use pnpm_deps_inspection::{
        TreeNodeId,
        build::importer_id_for,
        graph::{
            BuildGraphOptions,
            build_dependency_graph,
        },
    };
    let lockfile = Lockfile::load_current_from_install_state_dir(&state.config.install_state_dir)
        .into_diagnostic()
        .wrap_err("Loading the installed lockfile")?
        .ok_or_else(|| miette!("The installed lockfile is missing. Run pnpm install first"))?;
    let mut node = TreeNodeId::Importer(importer_id_for(state.lockfile_dir(), state.project_dir()));
    let options = BuildGraphOptions {
        lockfile: &lockfile,
        include: pnpm_modules_yaml::IncludedDependencies {
            dependencies: true,
            dev_dependencies: true,
            optional_dependencies: true,
        },
        only_projects: false,
        peer_edges: PeerEdgeOptions::default(),
    };
    let graph = build_dependency_graph(&[node.clone()], &options);
    for part in parts {
        node = graph.nodes
            .get(&node)
            .and_then(|node| {
                node.edges
                    .iter()
                    .find(|edge| &edge.alias == part)
            })
            .and_then(|edge| edge.target.clone())
            .ok_or_else(|| missing_from_lockfile(part))?;
    }
    let TreeNodeId::Package(key) = node else {
        return Err(miette!("The selected package is not an installed dependency"));
    };
    Ok(key)
}

fn missing_from_lockfile(part: &str) -> miette::Report {
    miette!(
        help = "Name a package that is not a direct dependency through the package that depends on it, such as <parent>/{part}",
        "Package '{part}' is missing from the installed lockfile"
    )
}

async fn rebuild_package<ReporterType: Reporter + 'static>(
    mut state: State,
    package_key: PackageKey,
) -> miette::Result<()> {
    let mut config = state.config.clone();
    // Edited files and build results must stay out of caches shared by other projects.
    config.side_effects_cache_read_setting = Some(false);
    config.side_effects_cache_write_setting = Some(false);
    config.remote_side_effects_cache = None;
    state.config = pnpm_config::Config::leak(config);
    let selection = super::rebuild::RebuildSelection {
        snapshots: Some(vec![package_key]),
        ..Default::default()
    };
    super::rebuild::run_rebuild::<ReporterType>(&state, selection, None).await
}

fn resolve_package_dir(
    modules_dir: &Path,
    parts: &[String],
    virtual_store_dir: &Path,
) -> miette::Result<PathBuf> {
    let modules_dir = dunce::canonicalize(modules_dir)
        .into_diagnostic()
        .wrap_err("Failed to canonicalize modules directory")?;
    let virtual_store_dir =
        dunce::canonicalize(virtual_store_dir).unwrap_or_else(|_| virtual_store_dir.to_path_buf());
    let mut current_dir = modules_dir.clone();
    for (i, part) in parts.iter().enumerate() {
        let candidate = if i == 0 {
            current_dir.join(part)
        } else if current_dir.starts_with(&virtual_store_dir) {
            isolated_dependency_dir(&current_dir, part)?
        } else {
            hoisted_dependency_dir(&current_dir, &modules_dir, part)
        };
        if !candidate.exists() {
            let dir = current_dir.display();
            return Err(miette!("Could not find package '{part}' under '{dir}'"));
        }
        current_dir = dunce::canonicalize(&candidate)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to canonicalize path '{}'", candidate.display()))?;
    }
    verify_package_path_within_tree(&current_dir, &modules_dir, &virtual_store_dir)?;
    Ok(current_dir)
}

/// pnpm's isolated layout puts a package and its dependency links beside
/// one another in `<slot>/node_modules`, not under the package directory.
fn isolated_dependency_dir(package_dir: &Path, dependency: &str) -> miette::Result<PathBuf> {
    package_dir
        .ancestors()
        .find(|ancestor| ancestor.file_name() == Some(std::ffi::OsStr::new("node_modules")))
        .map(|package_modules| package_modules.join(dependency))
        .ok_or_else(|| {
            miette!("Could not find package node_modules directory under isolated virtual store")
        })
}

/// Where Node.js finds `dependency` from `package_dir` in a hoisted layout:
/// the nearest `node_modules` between the package and `modules_dir` that has
/// it. Returns the package's own `node_modules` entry when none does.
fn hoisted_dependency_dir(package_dir: &Path, modules_dir: &Path, dependency: &str) -> PathBuf {
    let own = package_dir.join("node_modules").join(dependency);
    package_dir
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(modules_dir))
        .filter_map(|ancestor| {
            let name = ancestor.file_name()?;
            if ancestor == modules_dir || name == "node_modules" {
                Some(ancestor.join(dependency))
            } else if name.to_string_lossy().starts_with('@') {
                None
            } else {
                Some(ancestor.join("node_modules").join(dependency))
            }
        })
        .find(|candidate| candidate.exists())
        .unwrap_or(own)
}

fn resolve_package_dir_with_fallback(
    project_modules_dir: &Path,
    workspace_modules_dir: Option<&Path>,
    parts: &[String],
    virtual_store_dir: &Path,
) -> miette::Result<PathBuf> {
    resolve_package_dir(project_modules_dir, parts, virtual_store_dir)
        .or_else(|project_error| match workspace_modules_dir {
            Some(modules_dir) => resolve_package_dir(modules_dir, parts, virtual_store_dir),
            None => Err(project_error),
        })
}

fn verify_package_path_within_tree(
    path: &Path,
    modules_dir: &Path,
    virtual_store_dir: &Path,
) -> miette::Result<()> {
    let inside_modules = path.starts_with(modules_dir);
    let inside_virtual_store =
        dunce::canonicalize(virtual_store_dir).is_ok_and(|vs| path.starts_with(&vs));
    if !inside_modules && !inside_virtual_store {
        let dir = path.display();
        return Err(miette!(
            "Resolved package path '{dir}' is outside the expected node_modules tree"
        ));
    }
    Ok(())
}

fn resolve_editor_command(
    editor: &str,
    project_root: &Path,
    real_pkg_path: &Path,
) -> miette::Result<Command> {
    let editor_parts =
        shell_words::split(editor).into_diagnostic().wrap_err("Invalid editor command")?;
    let Some((program, args)) = editor_parts.split_first() else {
        return Err(miette!("No editor command specified"));
    };
    let program_path = find_safe_program_path(program, project_root)?;
    Ok(Command::new(program_path).with_args(args).with_arg(real_pkg_path))
}

fn find_safe_program_path(
    program: &str,
    project_root: &Path,
) -> miette::Result<std::ffi::OsString> {
    if !is_bare_name(program) {
        return Ok(program.into());
    }
    which::which_all(program)
        .ok()
        .and_then(|mut matches| {
            matches.find(|resolved| !path_is_within_project(resolved, project_root))
        })
        .map(std::path::PathBuf::into_os_string)
        .ok_or_else(|| miette!("Failed to execute editor command '{program}': no executable outside the project was found"))
}

fn parse_package_path(raw: &str) -> miette::Result<Vec<String>> {
    let segments: Vec<&str> = raw.split(['/', '\\']).collect();
    for seg in &segments {
        validate_package_path_segment(seg)?;
    }
    group_scoped_package_segments(&segments)
}

fn validate_package_path_segment(seg: &str) -> miette::Result<()> {
    if seg.is_empty() || seg == "." || seg == ".." || seg.contains(':') {
        return Err(miette!("Invalid package path segment: '{seg}'"));
    }
    Ok(())
}

fn group_scoped_package_segments(segments: &[&str]) -> miette::Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut seg_idx = 0;
    while seg_idx < segments.len() {
        let seg = segments[seg_idx];
        if seg.starts_with('@') {
            let Some(next_seg) = segments.get(seg_idx + 1) else {
                return Err(miette!("Incomplete scoped package name: '{seg}'"));
            };
            parts.push(format!("{seg}/{next_seg}"));
            seg_idx += 2;
        } else {
            parts.push(seg.to_string());
            seg_idx += 1;
        }
    }
    Ok(parts)
}

fn de_hardlink_dir(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_dir() {
            de_hardlink_dir(&path)?;
        } else if metadata.file_type().is_file() {
            de_hardlink_file(&path)?;
        }
    }
    Ok(())
}

fn de_hardlink_file(path: &Path) -> io::Result<()> {
    if !is_file_hardlinked(path)? {
        return Ok(());
    }
    let permissions = pnpm_fs::copy_permissions(path)?;
    #[cfg(all(unix, not(target_os = "wasi")))]
    let permissions = pnpm_fs::CopyPermissions::from_mode(permissions.mode() | 0o200);
    #[cfg(target_os = "wasi")]
    let permissions = pnpm_fs::CopyPermissions::from_mode(permissions.mode() | 0o200);
    pnpm_fs::copy_file_atomic_with_permissions(path, path, &permissions)
}

fn is_file_hardlinked(path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        fs::symlink_metadata(path).map(|metadata| metadata.nlink() > 1)
    }
    #[cfg(windows)]
    {
        pnpm_ffi_fs::file_information(path).map(|info| info.hard_links > 1)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(true)
    }
}

fn is_bare_name(name: &str) -> bool {
    !(name.contains('/') || cfg!(windows) && name.contains('\\'))
}

fn path_is_within_project(path: &Path, project_root: &Path) -> bool {
    dunce::canonicalize(path)
        .ok()
        .zip(dunce::canonicalize(project_root).ok())
        .is_some_and(|(path_canon, root_canon)| path_canon.starts_with(&root_canon))
}

#[cfg(test)]
mod tests;
