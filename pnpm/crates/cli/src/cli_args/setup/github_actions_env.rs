//! Persist `PNPM_HOME` and `$PNPM_HOME/bin` for the rest of a GitHub
//! Actions job.
//!
//! A workflow step gets a fresh shell, so the rc-file edit
//! [`super::path_extender::add_dir_to_env_path`] makes is invisible to
//! every later step. The runner instead reads back two line-oriented
//! files whose paths arrive in `GITHUB_ENV` and `GITHUB_PATH`, and
//! applies each record to the steps that follow.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::EnvVarOs;
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// Reject a value that cannot be persisted, before `setup` performs any
/// side effect.
pub(super) fn validate_persisted_values<Sys: EnvVarOs>(
    pnpm_home_dir: &Path,
    bin_dir: &Path,
) -> miette::Result<()> {
    if !should_persist::<Sys>() {
        return Ok(());
    }
    validate_value("PNPM_HOME", pnpm_home_dir)?;
    validate_value("pnpm setup bin directory", bin_dir)
}

/// Append `PNPM_HOME` to `GITHUB_ENV` and the bin directory to
/// `GITHUB_PATH`. A target that cannot be written is reported as a
/// warning and never stops the other one from being written.
pub(super) fn persist<Reporter: self::Reporter, Sys: EnvVarOs>(
    prefix_dir: &Path,
    pnpm_home_dir: &Path,
    bin_dir: &Path,
) {
    if !is_github_actions::<Sys>() {
        return;
    }
    let github_env = Sys::var_os("GITHUB_ENV").map(PathBuf::from);
    let github_path = Sys::var_os("GITHUB_PATH").map(PathBuf::from);
    write_files::<Reporter>(
        prefix_dir,
        pnpm_home_dir,
        bin_dir,
        github_env.as_deref(),
        github_path.as_deref(),
    );
}

fn should_persist<Sys: EnvVarOs>() -> bool {
    is_github_actions::<Sys>()
        && (Sys::var_os("GITHUB_ENV").is_some() || Sys::var_os("GITHUB_PATH").is_some())
}

fn is_github_actions<Sys: EnvVarOs>() -> bool {
    Sys::var_os("GITHUB_ACTIONS").is_some_and(|value| value == OsStr::new("true"))
}

/// `GITHUB_ENV` and `GITHUB_PATH` are line-oriented, so a line break in a
/// persisted value would append attacker-chosen records to the
/// environment of every later step in the workflow job.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("{name} cannot contain newline or NUL characters")]
#[diagnostic(code(ERR_PNPM_BAD_GITHUB_ACTIONS_ENVIRONMENT_VALUE))]
pub(super) struct BadValue {
    name: &'static str,
}

fn validate_value(name: &'static str, value: &Path) -> miette::Result<()> {
    if value.to_string_lossy().contains(['\n', '\r', '\0']) {
        return Err(BadValue { name }.into());
    }
    Ok(())
}

fn write_files<Reporter: self::Reporter>(
    prefix_dir: &Path,
    pnpm_home_dir: &Path,
    bin_dir: &Path,
    github_env: Option<&Path>,
    github_path: Option<&Path>,
) {
    if let Some(github_env) = github_env {
        append_file::<Reporter>(
            prefix_dir,
            "GITHUB_ENV",
            github_env,
            &format!("PNPM_HOME={}", pnpm_home_dir.display()),
        );
    }
    if let Some(github_path) = github_path {
        append_file::<Reporter>(
            prefix_dir,
            "GITHUB_PATH",
            github_path,
            &bin_dir.display().to_string(),
        );
    }
}

fn append_file<Reporter: self::Reporter>(
    prefix_dir: &Path,
    target_name: &str,
    path: &Path,
    line: &str,
) {
    if let Err(err) = append_existing_regular_file(path, line) {
        warn::<Reporter>(
            prefix_dir,
            &format!(
                "Failed to write GitHub Actions environment file {target_name} ({}): {err}",
                path.display(),
            ),
        );
    }
}

/// The runner creates both files up front, so anything but an existing
/// regular file at `path` is not the runner's target: skip it instead of
/// creating it or following a symlink to it.
fn append_existing_regular_file(path: &Path, line: &str) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    }
    let mut file = match open_existing_file_for_append(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if !file.metadata()?.file_type().is_file() {
        return Ok(());
    }
    write_line(&mut file, line)
}

fn open_existing_file_for_append(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).append(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    options.open(path)
}

/// Start a record of its own even when the runner left the file without
/// a trailing newline.
fn write_line(file: &mut File, line: &str) -> std::io::Result<()> {
    let mut output = String::new();
    if file.metadata()?.len() > 0 {
        let mut last_byte = [0];
        file.seek(SeekFrom::End(-1))?;
        file.read_exact(&mut last_byte)?;
        if last_byte[0] != b'\n' {
            output.push('\n');
        }
    }
    output.push_str(line);
    output.push('\n');
    file.write_all(output.as_bytes())
}

fn warn<Reporter: self::Reporter>(prefix: &Path, message: &str) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: message.to_string(),
        prefix: prefix.to_string_lossy().into_owned(),
    }));
}

#[cfg(test)]
mod tests;
