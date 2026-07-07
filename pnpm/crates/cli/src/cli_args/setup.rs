//! `pacquet setup` — make pnpm available for global use.
//!
//! The CLI is installed into the global packages directory, the `pn` /
//! `pnpx` / `pnx` alias scripts are written into `$PNPM_HOME/bin`, and
//! `PNPM_HOME` plus `$PNPM_HOME/bin` are added to the user's environment
//! (the shell rc file on POSIX, the registry on Windows).

mod path_extender;

use clap::Args;
use miette::{Context, IntoDiagnostic};
use pacquet_config::{Host, PNPM_VERSION, default_pnpm_home_dir};
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use path_extender::{
    AddDirToEnvPathOpts, AddingPosition, ConfigFileChangeType, ConfigReport, PathExtenderReport,
};
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

#[derive(Debug, Args)]
pub struct SetupArgs {
    /// Override the `PNPM_HOME` env variable in case it already exists.
    // `help` carries the verbatim pnpm wording (un-backticked) for `--help`,
    // while the doc comment keeps backticks for rustdoc.
    #[clap(
        long,
        short = 'f',
        help = "Override the PNPM_HOME env variable in case it already exists"
    )]
    pub force: bool,
}

impl SetupArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(self, dir: &Path) -> miette::Result<()> {
        let output = handler::<Reporter>(self.force, dir)?;
        println!("{output}");
        Ok(())
    }
}

fn handler<Reporter: self::Reporter + 'static>(force: bool, dir: &Path) -> miette::Result<String> {
    let pnpm_home_dir = default_pnpm_home_dir::<Host>().ok_or_else(|| {
        miette::miette!(
            "Could not determine the pnpm home directory. Set the PNPM_HOME environment variable."
        )
    })?;
    // Validate before any side effect: an unsafe `PNPM_HOME` must not reach
    // the self-install subprocess's `PATH` or the alias-script writes.
    path_extender::validate_pnpm_home_dir(&pnpm_home_dir)?;
    let bin_dir = pnpm_home_dir.join("bin");

    let exec_path = std::env::current_exe()
        .into_diagnostic()
        .wrap_err("determine the path to the pnpm executable")?;
    // pacquet is always a native executable (never a `.js` entrypoint), so
    // pnpm's single-executable branch always applies: install the CLI
    // globally and write the alias scripts.
    install_cli_globally::<Reporter>(&exec_path, &pnpm_home_dir, dir)?;
    create_alias_scripts(&bin_dir).into_diagnostic().wrap_err("create the pnpm alias scripts")?;

    let report = path_extender::add_dir_to_env_path(
        &pnpm_home_dir,
        &AddDirToEnvPathOpts {
            config_section_name: "pnpm",
            proxy_var_name: Some("PNPM_HOME"),
            proxy_var_sub_dir: Some("bin"),
            overwrite: force,
            position: AddingPosition::Start,
        },
    )?;
    persist_github_actions_environment::<Reporter>(dir, &pnpm_home_dir, &bin_dir)?;
    remove_legacy_homedir_shims(&pnpm_home_dir);
    Ok(render_setup_output(&report))
}

fn persist_github_actions_environment<Reporter: self::Reporter>(
    prefix_dir: &Path,
    pnpm_home_dir: &Path,
    bin_dir: &Path,
) -> miette::Result<()> {
    let is_github_actions =
        std::env::var_os("GITHUB_ACTIONS").is_some_and(|value| value == OsStr::new("true"));
    let github_env = std::env::var_os("GITHUB_ENV").map(PathBuf::from);
    let github_path = std::env::var_os("GITHUB_PATH").map(PathBuf::from);
    persist_github_actions_environment_to_files::<Reporter>(
        prefix_dir,
        is_github_actions,
        pnpm_home_dir,
        bin_dir,
        github_env.as_deref(),
        github_path.as_deref(),
    )
}

fn persist_github_actions_environment_to_files<Reporter: self::Reporter>(
    prefix_dir: &Path,
    is_github_actions: bool,
    pnpm_home_dir: &Path,
    bin_dir: &Path,
    github_env: Option<&Path>,
    github_path: Option<&Path>,
) -> miette::Result<()> {
    if !is_github_actions || (github_env.is_none() && github_path.is_none()) {
        return Ok(());
    }
    validate_github_actions_environment_file_value("PNPM_HOME", pnpm_home_dir)?;
    validate_github_actions_environment_file_value("pnpm setup bin directory", bin_dir)?;
    write_github_actions_environment_files::<Reporter>(
        prefix_dir,
        pnpm_home_dir,
        bin_dir,
        github_env,
        github_path,
    );
    Ok(())
}

fn validate_github_actions_environment_file_value(name: &str, value: &Path) -> miette::Result<()> {
    let value = value.to_string_lossy();
    if let Some(character) = value.chars().find(|character| matches!(character, '\n' | '\r' | '\0'))
    {
        return Err(miette::miette!(
            "{name} cannot contain newline or NUL characters: found {character:?}"
        ));
    }
    Ok(())
}

fn write_github_actions_environment_files<Reporter: self::Reporter>(
    prefix_dir: &Path,
    pnpm_home_dir: &Path,
    bin_dir: &Path,
    github_env: Option<&Path>,
    github_path: Option<&Path>,
) {
    if let Some(github_env) = github_env {
        append_github_actions_environment_file::<Reporter>(
            prefix_dir,
            "GITHUB_ENV",
            github_env,
            &format!("PNPM_HOME={}", pnpm_home_dir.display()),
        );
    }
    if let Some(github_path) = github_path {
        append_github_actions_environment_file::<Reporter>(
            prefix_dir,
            "GITHUB_PATH",
            github_path,
            &format!("{}", bin_dir.display()),
        );
    }
}

fn append_github_actions_environment_file<Reporter: self::Reporter>(
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
                path.display()
            ),
        );
    }
}

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
    write_line_to_file(&mut file, line)?;
    Ok(())
}

fn open_existing_file_for_append(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).append(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    options.open(path)
}

fn write_line_to_file(file: &mut File, line: &str) -> std::io::Result<()> {
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

/// Install the CLI as a global package using `pnpm add -g file:<dir>`,
/// placing it in the standard global directory alongside other globally
/// installed packages.
fn install_cli_globally<Reporter: self::Reporter + 'static>(
    exec_path: &Path,
    pnpm_home_dir: &Path,
    prefix_dir: &Path,
) -> miette::Result<()> {
    let exec_dir = exec_path
        .parent()
        .ok_or_else(|| miette::miette!("the pnpm executable has no parent directory"))?;
    let exec_name = exec_path
        .file_name()
        .ok_or_else(|| miette::miette!("the pnpm executable has no file name"))?
        .to_string_lossy()
        .into_owned();
    let pkg_json_path = exec_dir.join("package.json");

    // Write a package.json if one doesn't already exist. (Updated tarballs
    // ship with package.json already.)
    let created_pkg_json = !pkg_json_path.exists();
    if created_pkg_json {
        let pkg = serde_json::json!({
            "name": "@pnpm/exe",
            "version": PNPM_VERSION,
            "bin": { "pnpm": exec_name, "pn": exec_name },
        });
        fs::write(&pkg_json_path, pkg.to_string())
            .into_diagnostic()
            .wrap_err("write the temporary package.json next to the pnpm executable")?;
    }

    info::<Reporter>(
        &prefix_dir.to_string_lossy(),
        &format!("Installing pnpm CLI globally from {}", exec_dir.display()),
    );

    // `@pnpm/exe` ships a preinstall/prepare pair that hardlinks the
    // platform-specific binary out of its optional platform packages. None
    // of that applies here: this `file:` dependency is the standalone
    // executable itself, the platform packages aren't installed alongside
    // it, and the host may have no `node` to run the scripts. Skipping them
    // also avoids a build-approval prompt for pnpm's own install.
    let separator = if cfg!(windows) { ";" } else { ":" };
    // Build `PATH` as an `OsString` so a non-UTF-8 ambient `PATH` is
    // preserved verbatim rather than lost to a lossy string conversion.
    let mut path_value = pnpm_home_dir.join("bin").into_os_string();
    path_value.push(separator);
    if let Some(existing) = std::env::var_os("PATH") {
        path_value.push(existing);
    }
    let status = Command::new(exec_path)
        .args(["add", "-g", "--ignore-scripts", &format!("file:{}", exec_dir.display())])
        .env("PNPM_HOME", pnpm_home_dir)
        .env("PATH", path_value)
        .status();

    // Always attempt the cleanup, but let the install error take precedence
    // over a cleanup error.
    let cleanup = if created_pkg_json { fs::remove_file(&pkg_json_path) } else { Ok(()) };

    let status = status.into_diagnostic().wrap_err("run the global pnpm install")?;
    if !status.success() {
        let code = status.code().map_or_else(|| "unknown".to_string(), |code| code.to_string());
        return Err(miette::miette!("Failed to install pnpm globally (exit code {code})"));
    }
    cleanup
        .into_diagnostic()
        .wrap_err("remove the temporary package.json next to the pnpm executable")?;
    Ok(())
}

/// Write the `pn` / `pnpx` / `pnx` wrapper scripts into `$PNPM_HOME/bin`.
///
/// Script files are used instead of shell aliases because aliases don't work
/// across all shells (Windows `cmd`, POSIX `sh`) or environments
/// (non-interactive, CI), can't be located with `which` / `where`, and
/// editing rc files is more error-prone than writing files.
fn create_alias_scripts(target_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(target_dir)?;
    create_shell_script(target_dir, "pn", "pnpm")?;
    create_shell_script(target_dir, "pnpx", "pnpm dlx")?;
    create_shell_script(target_dir, "pnx", "pnpm dlx")?;
    Ok(())
}

fn create_shell_script(target_dir: &Path, name: &str, command: &str) -> std::io::Result<()> {
    // Windows can also run shell scripts via mingw / cygwin, so write the
    // POSIX script unconditionally.
    let shell_script = format!("#!/bin/sh\nexec {command} \"$@\"\n");
    let script_path = target_dir.join(name);
    fs::write(&script_path, shell_script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))?;
    }

    if cfg!(windows) {
        fs::write(target_dir.join(format!("{name}.cmd")), format!("@echo off\n{command} %*\n"))?;
        fs::write(target_dir.join(format!("{name}.ps1")), format!("{command} @args\n"))?;
    }
    Ok(())
}

/// v10-layout shim names that v11 writes under `pnpm_home_dir/bin` instead.
const LEGACY_HOME_DIR_SHIM_NAMES: &[&str] = &[
    "pnpm", "pnpm.cmd", "pnpm.ps1", "pn", "pn.cmd", "pn.ps1", "pnpx", "pnpx.cmd", "pnpx.ps1",
    "pnx", "pnx.cmd", "pnx.ps1",
];

fn remove_legacy_homedir_shims(pnpm_home_dir: &Path) {
    for name in LEGACY_HOME_DIR_SHIM_NAMES {
        // A leftover shim is harmless once PATH points at bin/, so failure here is fine.
        let _ = fs::remove_file(pnpm_home_dir.join(name));
    }
}

/// Render the user-facing summary of what changed.
fn render_setup_output(report: &PathExtenderReport) -> String {
    if report.old_settings == report.new_settings {
        return "No changes to the environment were made. Everything is already up to date."
            .to_string();
    }
    let mut output = Vec::new();
    if let Some(config_file) = &report.config_file {
        output.push(report_config_change(config_file));
    }
    output.push(format!("Next configuration changes were made:\n{}", report.new_settings));
    match &report.config_file {
        None => output.push("Setup complete. Open a new terminal to start using pnpm.".to_string()),
        Some(config_file) if config_file.change_type != ConfigFileChangeType::Skipped => output
            .push(format!("To start using pnpm, run:\nsource {}\n", config_file.path.display())),
        Some(_) => {}
    }
    output.join("\n\n")
}

fn report_config_change(config_report: &ConfigReport) -> String {
    let path = config_report.path.display();
    match config_report.change_type {
        ConfigFileChangeType::Created => format!("Created {path}"),
        ConfigFileChangeType::Appended => format!("Appended new lines to {path}"),
        ConfigFileChangeType::Modified => format!("Replaced configuration in {path}"),
        ConfigFileChangeType::Skipped => format!("Configuration already up to date in {path}"),
    }
}

fn info<Reporter: self::Reporter>(prefix: &str, message: &str) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: message.to_string(),
        prefix: prefix.to_string(),
    }));
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
