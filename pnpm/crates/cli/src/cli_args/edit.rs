use crate::state::State;
use clap::Args;
use miette::{Context, IntoDiagnostic, miette};
use pnpm_reporter::Reporter;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

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
        let modules_dir = dunce::canonicalize(&state.config.modules_dir)
            .into_diagnostic()
            .wrap_err("Failed to canonicalize modules directory")?;
        let real_pkg_path =
            resolve_package_dir(&modules_dir, &parts, state.config.virtual_store_dir())?;

        de_hardlink_dir(&real_pkg_path)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to break store hard links for editing in '{}'",
                    real_pkg_path.display(),
                )
            })?;

        let editor = self.pick_editor(&state);
        let mut command = resolve_editor_command(&editor, &modules_dir, &real_pkg_path)?;
        let status = command
            .status()
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to execute editor command: {editor}"))?;

        if !status.success() {
            return Err(miette!("Editor command exited with failure status"));
        }

        if let Some(pkg_to_rebuild) = parts.last() {
            let selection = super::rebuild::RebuildSelection {
                names: Some(vec![pkg_to_rebuild.clone()]),
                projects: Vec::new(),
            };
            super::rebuild::run_rebuild::<ReporterType>(&state, selection, None).await?;
        }

        Ok(())
    }

    fn pick_editor(&self, state: &State) -> String {
        self.editor
            .clone()
            .or_else(|| state.config.editor.clone())
            .or_else(|| env::var("EDITOR").ok())
            .or_else(|| env::var("VISUAL").ok())
            .unwrap_or_else(|| if cfg!(windows) { "notepad".to_owned() } else { "vi".to_owned() })
    }
}

fn resolve_package_dir(
    modules_dir: &Path,
    parts: &[String],
    virtual_store_dir: &Path,
) -> miette::Result<PathBuf> {
    let mut current_dir = modules_dir.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        let candidate = if i == 0 {
            current_dir.join(part)
        } else {
            current_dir.join("node_modules").join(part)
        };
        if !candidate.exists() {
            let dir = current_dir.display();
            return Err(miette!("Could not find package '{part}' under '{dir}'"));
        }
        current_dir = dunce::canonicalize(&candidate)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to canonicalize path '{}'", candidate.display()))?;
    }
    verify_package_path_within_tree(&current_dir, modules_dir, virtual_store_dir)?;
    Ok(current_dir)
}

fn verify_package_path_within_tree(
    path: &Path,
    modules_dir: &Path,
    virtual_store_dir: &Path,
) -> miette::Result<()> {
    let inside_modules = path.starts_with(modules_dir);
    let inside_virtual_store = dunce::canonicalize(virtual_store_dir)
        .ok()
        .is_some_and(|vs| path.starts_with(&vs));
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
    modules_dir: &Path,
    real_pkg_path: &Path,
) -> miette::Result<Command> {
    let editor_parts = split_shell_args(editor);
    let Some((program, args)) = editor_parts.split_first() else {
        return Err(miette!("No editor command specified"));
    };
    let program_path = find_safe_program_path(program, modules_dir);
    let mut command = Command::new(program_path);
    command.args(args);
    command.arg(real_pkg_path);
    Ok(command)
}

fn find_safe_program_path(program: &str, modules_dir: &Path) -> std::ffi::OsString {
    if !is_bare_name(program) {
        return program.into();
    }
    which::which_all(program)
        .ok()
        .and_then(|mut matches| {
            matches.find(|resolved| !path_is_within_project(resolved, modules_dir))
        })
        .map_or_else(|| program.into(), std::path::PathBuf::into_os_string)
}

fn split_shell_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_double_quote = false;
    let mut in_single_quote = false;
    let mut chars = input.chars().peekable();

    while let Some(character) = chars.next() {
        process_shell_char(
            character,
            &mut chars,
            &mut in_double_quote,
            &mut in_single_quote,
            &mut current,
            &mut args,
        );
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn process_shell_char(
    character: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    in_double_quote: &mut bool,
    in_single_quote: &mut bool,
    current: &mut String,
    args: &mut Vec<String>,
) {
    if character == '\\' && !*in_single_quote {
        if let Some('\\' | '"' | '\'' | ' ' | '\t') = chars.peek() {
            if let Some(escaped) = chars.next() {
                current.push(escaped);
            }
            return;
        }
        current.push(character);
    } else if character == '"' && !*in_single_quote {
        *in_double_quote = !*in_double_quote;
    } else if character == '\'' && !*in_double_quote {
        *in_single_quote = !*in_single_quote;
    } else if character.is_whitespace() && !*in_double_quote && !*in_single_quote {
        if !current.is_empty() {
            args.push(std::mem::take(current));
        }
    } else {
        current.push(character);
    }
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
            de_hardlink_file(&path, &metadata)?;
        }
    }
    Ok(())
}

fn de_hardlink_file(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    if !is_file_hardlinked(metadata) {
        return Ok(());
    }
    let owner_writable = make_owner_writable(&metadata.permissions());
    let parent_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent_dir)?;
    {
        let mut src = fs::File::open(path)?;
        std::io::copy(&mut src, &mut tmp)?;
    }
    tmp.as_file().sync_all()?;
    tmp.as_file().set_permissions(owner_writable)?;
    std::fs::remove_file(path)?;
    tmp.persist(path).map_err(|err| err.error)?;
    Ok(())
}

fn is_file_hardlinked(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        metadata.nlink() > 1
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        true
    }
}

fn make_owner_writable(permissions: &fs::Permissions) -> fs::Permissions {
    #[cfg(unix)]
    {
        let mode = permissions.mode();
        fs::Permissions::from_mode(mode | 0o200)
    }
    #[cfg(not(unix))]
    {
        let mut p = permissions.clone();
        p.set_readonly(false);
        p
    }
}

fn is_bare_name(name: &str) -> bool {
    !(name.contains('/') || cfg!(windows) && name.contains('\\'))
}

fn path_is_within_project(path: &Path, project_root: &Path) -> bool {
    dunce::canonicalize(path)
        .ok()
        .zip(dunce::canonicalize(project_root).ok())
        .is_some_and(|(p, r)| p.starts_with(&r))
}

#[cfg(test)]
mod tests;
