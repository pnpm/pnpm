use crate::state::State;
use clap::Args;
use miette::{Context, IntoDiagnostic, miette};
use pacquet_reporter::Reporter;
use std::{env, fs, io, path::Path, process::Command};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

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
        let package_path = &self.package_path;

        // Parse package name and potential sub-packages
        let parts = parse_package_path(package_path)?;

        let modules_dir = state.config.modules_dir.clone();
        let mut current_dir = modules_dir.clone();
        for (i, part) in parts.iter().enumerate() {
            let candidate = if i == 0 {
                current_dir.join(part)
            } else {
                current_dir.join("node_modules").join(part)
            };
            if !candidate.exists() {
                let dir = current_dir.display();
                return Err(miette!("Could not find package '{}' under '{}'", part, dir));
            }
            // Follow symlinks to the real directory in virtual store
            current_dir = fs::canonicalize(&candidate).into_diagnostic().wrap_err_with(|| {
                format!("Failed to canonicalize path '{}'", candidate.display())
            })?;
        }

        let real_pkg_path = current_dir;

        // Verify the resolved path stays under the expected modules directory tree
        if !real_pkg_path.starts_with(&modules_dir) {
            let dir = real_pkg_path.display();
            return Err(miette!(
                "Resolved package path '{}' is outside the expected node_modules tree",
                dir
            ));
        }

        // De-hardlink the package files before editing to protect the central store from corruption!
        de_hardlink_dir(&real_pkg_path).into_diagnostic().wrap_err_with(|| {
            format!("Failed to break store hard links for editing in '{}'", real_pkg_path.display())
        })?;

        // Determine which editor to use: CLI flag, config, env var, or default
        let editor = self
            .editor
            .or_else(|| state.config.editor.clone())
            .or_else(|| env::var("EDITOR").ok())
            .or_else(|| env::var("VISUAL").ok())
            .unwrap_or_else(|| if cfg!(windows) { "notepad".to_owned() } else { "vi".to_owned() });

        // Launch editor directly without shell wrapping to avoid command injection
        let editor_parts = split_shell_args(&editor);
        if editor_parts.is_empty() {
            return Err(miette!("No editor command specified"));
        }
        let program = &editor_parts[0];
        let mut command = Command::new(program);
        if editor_parts.len() > 1 {
            command.args(&editor_parts[1..]);
        }
        command.arg(&real_pkg_path);

        let status = command
            .status()
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to execute editor command: {editor}"))?;

        if !status.success() {
            return Err(miette!("Editor command exited with failure status"));
        }

        // Rebuild the package
        if let Some(pkg_to_rebuild) = parts.last() {
            super::rebuild::run_rebuild::<ReporterType>(&state, Some(vec![pkg_to_rebuild.clone()]))
                .await?;
        }

        Ok(())
    }
}

fn split_shell_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_double_quote = false;
    let mut in_single_quote = false;

    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' && !in_single_quote {
            if let Some('\\' | '"' | '\'' | ' ' | '\t') = chars.peek() {
                current.push(chars.next().unwrap());
                continue;
            }
            current.push(character);
            continue;
        } else if character == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
        } else if character == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
        } else if character.is_whitespace() && !in_double_quote && !in_single_quote {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn parse_package_path(raw: &str) -> miette::Result<Vec<String>> {
    let mut parts = Vec::new();
    let segments: Vec<&str> = raw.split(['/', '\\']).collect();
    for seg in &segments {
        if seg.is_empty() || *seg == "." || *seg == ".." || seg.contains(':') {
            return Err(miette!("Invalid package path segment: '{}'", seg));
        }
    }
    let mut seg_idx = 0;
    while seg_idx < segments.len() {
        let seg = segments[seg_idx];
        if seg.starts_with('@') {
            if seg_idx + 1 < segments.len() {
                parts.push(format!("{}/{}", seg, segments[seg_idx + 1]));
                seg_idx += 2;
            } else {
                return Err(miette!("Incomplete scoped package name: '{}'", seg));
            }
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
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            de_hardlink_dir(&path)?;
        } else if file_type.is_file() {
            let is_hardlinked = {
                #[cfg(unix)]
                {
                    metadata.nlink() > 1
                }
                #[cfg(windows)]
                {
                    // number_of_links() is unstable on stable Windows (requires
                    // nightly feature `windows_by_handle`). Assume all files are
                    // hardlinked to safely de-hardlink them anyway.
                    true
                }
                #[cfg(not(any(unix, windows)))]
                {
                    true
                }
            };
            if !is_hardlinked {
                continue;
            }

            let original_permissions = metadata.permissions();
            let owner_writable_permissions = make_owner_writable(&original_permissions);

            let parent_dir = path.parent().unwrap_or_else(|| Path::new("."));
            let mut tmp = tempfile::NamedTempFile::new_in(parent_dir)?;
            {
                let mut src = fs::File::open(&path)?;
                std::io::copy(&mut src, &mut tmp)?;
            }
            tmp.as_file().sync_all()?;

            tmp.as_file().set_permissions(owner_writable_permissions)?;

            tmp.persist(&path).map_err(|err| err.error)?;
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests;
