//! Put the package manager a git-hosted dependency needs on the `PATH`
//! its build runs with.
//!
//! The dependency's install and prepublish scripts invoke `yarn`, `npm` or
//! `bun` by name — its own scripts do too — so the package manager has to
//! be reachable as a command, not just as a path pnpm knows. Each shim
//! forwards to `pnpm dlx <pm>@<spec>`, which resolves, verifies and
//! installs that package manager once and reuses it afterwards.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::preferred_pm::WantedPm;

/// The shims written for a package manager: the command it is invoked as,
/// plus the aliases its own scripts may reach for.
pub(crate) fn shim_names(pm: crate::preferred_pm::PreferredPm) -> &'static [&'static str] {
    match pm {
        crate::preferred_pm::PreferredPm::Bun => &["bun"],
        crate::preferred_pm::PreferredPm::Npm => &["npm"],
        crate::preferred_pm::PreferredPm::Pnpm => &["pnpm"],
        crate::preferred_pm::PreferredPm::Yarn => &["yarn", "yarnpkg"],
    }
}

/// Write shims for `wanted` into `dir`, which the caller prepends to the
/// build's `PATH`. `pnpm_execpath` is the running pnpm, invoked by
/// absolute path because the build's `PATH` is not guaranteed to hold one.
pub(crate) fn write_pm_shims(
    dir: &Path,
    wanted: &WantedPm,
    pnpm_execpath: &Path,
) -> io::Result<Vec<PathBuf>> {
    fs::create_dir_all(dir)?;
    let spec = match wanted.version_spec.as_deref().and_then(command_line_safe) {
        Some(version_spec) => format!("{}@{version_spec}", wanted.pm.name()),
        None => wanted.pm.name().to_string(),
    };
    let mut written = Vec::new();
    for name in shim_names(wanted.pm) {
        for (file_name, contents) in shim_files(name, &spec, pnpm_execpath) {
            let path = dir.join(file_name);
            write_executable(&path, &contents)?;
            written.push(path);
        }
    }
    Ok(written)
}

#[cfg(unix)]
fn shim_files(name: &str, spec: &str, pnpm_execpath: &Path) -> Vec<(String, String)> {
    let contents = format!(
        "#!/bin/sh\nexec {pnpm} dlx {spec} \"$@\"\n",
        pnpm = sh_quote(&pnpm_execpath.to_string_lossy()),
        spec = sh_quote(spec),
    );
    vec![(name.to_string(), contents)]
}

#[cfg(windows)]
fn shim_files(name: &str, spec: &str, pnpm_execpath: &Path) -> Vec<(String, String)> {
    let pnpm = pnpm_execpath.display();
    let contents = format!("@\"{pnpm}\" dlx \"{spec}\" %*\r\n");
    vec![(format!("{name}.cmd"), contents)]
}

/// `version_spec` if every character of it can appear in a semver range,
/// and `None` otherwise — the version is then left to the channel default.
///
/// `cmd.exe` has no way to escape a quote inside a quoted argument, so the
/// Windows shim cannot make an arbitrary specifier safe by quoting it. The
/// specifier originates in a dependency's manifest, so it is checked here,
/// at the point where it becomes part of a command line, and not only
/// where [`crate::preferred_pm`] parses it.
fn command_line_safe(version_spec: &str) -> Option<&str> {
    const RANGE_PUNCTUATION: &str = ".-+^~<>=*| ,";
    version_spec
        .chars()
        .all(|char| char.is_ascii_alphanumeric() || RANGE_PUNCTUATION.contains(char))
        .then_some(version_spec)
}

/// Single-quote a value for `sh`, so a path holding a space or a shell
/// metacharacter cannot change what the shim runs.
#[cfg(unix)]
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Write `contents` to `path`, replacing whatever was there. The shims sit
/// in pnpm's own temporary area, but they are about to be prepended to a
/// build's `PATH`: removing first means a pre-existing symlink at the path
/// cannot redirect the write, and the file that ends up executed is always
/// the one just generated.
fn write_executable(path: &Path, contents: &str) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
