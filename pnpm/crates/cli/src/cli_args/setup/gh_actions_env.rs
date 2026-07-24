//! Persist `PNPM_HOME` and `$PNPM_HOME/bin` for the rest of a GitHub
//! Actions job.
//!
//! Every workflow step gets a fresh shell, so the rc-file edit
//! [`super::path_extender::add_dir_to_env_path`] makes is invisible to the
//! steps that follow. The runner instead reads back two line-oriented
//! files, named by `GITHUB_ENV` and `GITHUB_PATH`, and applies each record
//! to the rest of the job.
//!
//! Both files belong to the runner, which creates them up front. A path
//! that holds anything else is not the runner's target. A missing file, a
//! symlink, or a directory is therefore left alone rather than created or
//! followed.

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

/// Called before `setup` performs any side effect, so an unusable value
/// aborts the command instead of half-completing it.
pub(super) fn validate_gh_actions_env_file_values<Sys: EnvVarOs>(
    pnpm_home_dir: &Path,
    bin_dir: &Path,
) -> miette::Result<()> {
    if !should_write_gh_actions_env_files::<Sys>() {
        return Ok(());
    }
    validate_gh_actions_env_file_value("PNPM_HOME", pnpm_home_dir)?;
    validate_gh_actions_env_file_value("pnpm setup bin directory", bin_dir)
}

/// A target that cannot be written is reported as a warning rather than
/// failing the command. The shell config is already updated by this point,
/// so the setup itself succeeded.
pub(super) fn write_gh_actions_env_files<Reporter: self::Reporter, Sys: EnvVarOs>(
    prefix_dir: &Path,
    pnpm_home_dir: &Path,
    bin_dir: &Path,
) {
    if !is_gh_actions::<Sys>() {
        return;
    }
    let github_env = Sys::var_os("GITHUB_ENV").map(PathBuf::from);
    let github_path = Sys::var_os("GITHUB_PATH").map(PathBuf::from);
    append_gh_actions_env_files::<Reporter>(
        prefix_dir,
        pnpm_home_dir,
        bin_dir,
        github_env.as_deref(),
        github_path.as_deref(),
    );
}

fn should_write_gh_actions_env_files<Sys: EnvVarOs>() -> bool {
    is_gh_actions::<Sys>()
        && (Sys::var_os("GITHUB_ENV").is_some() || Sys::var_os("GITHUB_PATH").is_some())
}

fn is_gh_actions<Sys: EnvVarOs>() -> bool {
    Sys::var_os("GITHUB_ACTIONS").is_some_and(|value| value == OsStr::new("true"))
}

/// The files are line-oriented, so a line break in a persisted value would
/// append attacker-chosen records to the environment of every later step.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("{name} cannot contain newline or NUL characters")]
#[diagnostic(code(ERR_PNPM_BAD_GITHUB_ACTIONS_ENVIRONMENT_VALUE))]
pub(super) struct BadGhActionsEnvFileValue {
    name: &'static str,
}

fn validate_gh_actions_env_file_value(name: &'static str, value: &Path) -> miette::Result<()> {
    if value.to_string_lossy().contains(['\n', '\r', '\0']) {
        return Err(BadGhActionsEnvFileValue { name }.into());
    }
    Ok(())
}

fn append_gh_actions_env_files<Reporter: self::Reporter>(
    prefix_dir: &Path,
    pnpm_home_dir: &Path,
    bin_dir: &Path,
    github_env: Option<&Path>,
    github_path: Option<&Path>,
) {
    if let Some(github_env) = github_env {
        append_gh_actions_env_file::<Reporter>(
            prefix_dir,
            "GITHUB_ENV",
            github_env,
            &format!("PNPM_HOME={}", pnpm_home_dir.display()),
        );
    }
    if let Some(github_path) = github_path {
        append_gh_actions_env_file::<Reporter>(
            prefix_dir,
            "GITHUB_PATH",
            github_path,
            &bin_dir.display().to_string(),
        );
    }
}

fn append_gh_actions_env_file<Reporter: self::Reporter>(
    prefix_dir: &Path,
    target_name: &str,
    path: &Path,
    line: &str,
) {
    if let Err(err) = append_line_to_regular_file(path, line) {
        warn::<Reporter>(
            prefix_dir,
            &format!(
                "Failed to write GitHub Actions environment file {target_name} ({}): {err}",
                path.display(),
            ),
        );
    }
}

fn append_line_to_regular_file(path: &Path, line: &str) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    }
    let mut file = match open_for_append(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    // The `symlink_metadata` above races with anything that swaps the path
    // between the two syscalls, so re-check through the descriptor.
    if !file.metadata()?.file_type().is_file() {
        return Ok(());
    }
    write_line(&mut file, line)
}

fn open_for_append(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).append(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    options.open(path)
}

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
