//! Put the package manager a git-hosted dependency needs on the `PATH`
//! its build runs with.
//!
//! The dependency's install and prepublish scripts invoke `yarn`, `npm` or
//! `bun` by name — its own scripts do too — so the package manager has to
//! be reachable as a command, not just as a path pnpm knows. Each shim
//! forwards to `pnpm dlx --package <pm>@<spec> <command>`, which resolves,
//! verifies and installs that package manager once, reuses it afterwards,
//! and runs the command the script actually asked for: `npx` is npm's
//! other bin, not another name for it.

use pnpm_fs::write_atomic;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::preferred_pm::WantedPm;

/// The commands a package manager answers to, all of which a dependency's
/// scripts may reach for, and how each one is run.
///
/// A command is usually a bin of the engine itself — the alias of the same
/// executable (`yarnpkg`), or the separate command it publishes beside
/// itself (`npx`). Bun ships one executable and answers to `bunx` as
/// `bun x`, so that command carries the subcommand instead.
pub(crate) fn shim_commands(
    pm: crate::preferred_pm::PreferredPm,
) -> &'static [(&'static str, &'static [&'static str])] {
    use crate::preferred_pm::PreferredPm;
    match pm {
        PreferredPm::Bun => &[("bun", &[]), ("bunx", &["bun", "x"])],
        PreferredPm::Npm => &[("npm", &[]), ("npx", &[])],
        PreferredPm::Pnpm => &[("pnpm", &[])],
        PreferredPm::Yarn => &[("yarn", &[]), ("yarnpkg", &[])],
    }
}

/// The names those commands are reachable by, which is what a host has to
/// provide for pnpm to leave the job to it.
pub(crate) fn shim_names(
    pm: crate::preferred_pm::PreferredPm,
) -> impl Iterator<Item = &'static str> {
    shim_commands(pm).iter().map(|(name, _)| *name)
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
    for (name, run_as) in shim_commands(wanted.pm) {
        // `bunx` runs `bun x`; every other command runs the one it is
        // named after.
        let run_as: Vec<&str> = if run_as.is_empty() { vec![name] } else { run_as.to_vec() };
        for (file_name, contents) in shim_files(name, &run_as, &spec, pnpm_execpath) {
            let path = dir.join(file_name);
            write_executable(&path, &contents)?;
            written.push(path);
        }
    }
    Ok(written)
}

#[cfg(unix)]
fn shim_files(
    name: &str,
    run_as: &[&str],
    spec: &str,
    pnpm_execpath: &Path,
) -> Vec<(String, String)> {
    use pnpm_cmd_shim::sh_single_quote;

    let run_as: Vec<String> = run_as.iter().map(|word| sh_single_quote(word)).collect();
    let contents = format!(
        "#!/bin/sh\nexec {pnpm} dlx --package {spec} {run_as} \"$@\"\n",
        pnpm = sh_single_quote(&pnpm_execpath.to_string_lossy()),
        spec = sh_single_quote(spec),
        run_as = run_as.join(" "),
    );
    vec![(name.to_string(), contents)]
}

#[cfg(windows)]
fn shim_files(
    name: &str,
    run_as: &[&str],
    spec: &str,
    pnpm_execpath: &Path,
) -> Vec<(String, String)> {
    use pnpm_cmd_shim::cmd_escape;

    let pnpm = cmd_escape(&pnpm_execpath.to_string_lossy());
    let spec = cmd_escape(spec);
    let run_as: Vec<String> =
        run_as.iter().map(|word| format!(r#""{}""#, cmd_escape(word))).collect();
    let run_as = run_as.join(" ");
    let contents = format!("@\"{pnpm}\" dlx --package \"{spec}\" {run_as} %*\r\n");
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

/// Write `contents` to `path`, replacing whatever was there.
///
/// The shims sit in pnpm's own temporary area, but they are about to be
/// prepended to a build's `PATH`: the replacement is a rename, so an entry
/// planted at the path can neither redirect the write nor be the thing
/// that ends up executed, and no window exists in which the path holds
/// something half-written.
fn write_executable(path: &Path, contents: &str) -> io::Result<()> {
    write_atomic(path, contents.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
