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

use crate::{ProvisionedTool, preferred_pm::WantedPm};

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

/// Write shims into `dir` that run each command of `pm` from `tool`, the
/// package manager the lockfile records. A tool whose commands are scripts
/// runs on `node`, or on the host's `node` when it is `None`.
pub(crate) fn write_locked_pm_shims(
    dir: &Path,
    pm: crate::preferred_pm::PreferredPm,
    tool: &ProvisionedTool,
    node: Option<&Path>,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for (name, run_as) in shim_commands(pm) {
        let (command, args) = run_as
            .split_first()
            .unwrap_or((name, &[]));
        let target = tool
            .command(command)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{}@{} publishes no {command} command", tool.name, tool.version),
                )
            })?;
        let launcher = match (tool.runs_on_node, node) {
            (false, _) => Launcher::Direct,
            (true, None) => Launcher::HostNode,
            (true, Some(node)) => Launcher::Node(node),
        };
        let program = LockedProgram { launcher, target, args };
        let (file_name, contents) = locked_shim_file(name, &program);
        write_executable(&dir.join(file_name), &contents)?;
    }
    Ok(())
}

/// What a locked shim runs: `target` with `args` in front of the caller's.
struct LockedProgram<'a> {
    launcher: Launcher<'a>,
    target: &'a Path,
    args: &'a [&'a str],
}

/// How a locked shim starts its target.
#[derive(Clone, Copy)]
enum Launcher<'a> {
    /// The target is an executable.
    Direct,
    /// The target is a script for whatever `node` the build's `PATH` has.
    HostNode,
    /// The target is a script for this Node.js.
    Node(&'a Path),
}

#[cfg(any(unix, target_os = "wasi"))]
fn locked_shim_file(name: &str, program: &LockedProgram<'_>) -> (String, String) {
    use pnpm_cmd_shim::sh_single_quote;

    let mut words: Vec<String> = Vec::new();
    match program.launcher {
        Launcher::Node(node) => words.push(sh_single_quote(&node.to_string_lossy())),
        Launcher::HostNode => words.push("node".to_string()),
        Launcher::Direct => {}
    }
    words.push(sh_single_quote(&program.target.to_string_lossy()));
    words.extend(program.args.iter().map(|arg| sh_single_quote(arg)));
    (name.to_string(), format!("#!/bin/sh\nexec {} \"$@\"\n", words.join(" ")))
}

#[cfg(windows)]
fn locked_shim_file(name: &str, program: &LockedProgram<'_>) -> (String, String) {
    use pnpm_cmd_shim::cmd_escape;

    let mut words: Vec<String> = Vec::new();
    match program.launcher {
        Launcher::Node(node) => words.push(format!(r#""{}""#, cmd_escape(&node.to_string_lossy()))),
        Launcher::HostNode => words.push("node".to_string()),
        Launcher::Direct => {}
    }
    words.push(format!(r#""{}""#, cmd_escape(&program.target.to_string_lossy())));
    words.extend(
        program.args
            .iter()
            .map(|arg| format!(r#""{}""#, cmd_escape(arg))),
    );
    (format!("{name}.cmd"), format!("@{} %*\r\n", words.join(" ")))
}

/// Put the running pnpm on the build's `PATH` as `pnpm`, returning the
/// scratch directory its shim lives in.
///
/// Without the running pnpm, or when the shim cannot be written, the
/// host's pnpm prepares the dependency.
pub(crate) fn provide_running_pnpm(pnpm_execpath: Option<&Path>) -> Option<tempfile::TempDir> {
    let pnpm_execpath = pnpm_execpath?;
    let provided = tempfile::tempdir()
        .and_then(|dir| {
            write_running_pnpm_shim(dir.path(), pnpm_execpath)?;
            Ok(dir)
        });
    provided
        .inspect_err(|error| {
            tracing::warn!(
                target: "pacquet::git_fetcher",
                "could not provide the running pnpm for the build: {error}",
            );
        })
        .ok()
}

/// Write a `pnpm` shim into `dir` that runs `pnpm_execpath` itself.
fn write_running_pnpm_shim(dir: &Path, pnpm_execpath: &Path) -> io::Result<PathBuf> {
    let (file_name, contents) = running_pnpm_shim_file(pnpm_execpath);
    let path = dir.join(file_name);
    write_executable(&path, &contents)?;
    Ok(path)
}

#[cfg(any(unix, target_os = "wasi"))]
fn running_pnpm_shim_file(pnpm_execpath: &Path) -> (String, String) {
    use pnpm_cmd_shim::sh_single_quote;

    let pnpm = sh_single_quote(&pnpm_execpath.to_string_lossy());
    ("pnpm".to_string(), format!("#!/bin/sh\nexec {pnpm} \"$@\"\n"))
}

#[cfg(windows)]
fn running_pnpm_shim_file(pnpm_execpath: &Path) -> (String, String) {
    use pnpm_cmd_shim::cmd_escape;

    let pnpm = cmd_escape(&pnpm_execpath.to_string_lossy());
    ("pnpm.cmd".to_string(), format!("@\"{pnpm}\" %*\r\n"))
}

#[cfg(any(unix, target_os = "wasi"))]
fn shim_files(
    name: &str,
    run_as: &[&str],
    spec: &str,
    pnpm_execpath: &Path,
) -> Vec<(String, String)> {
    use pnpm_cmd_shim::sh_single_quote;

    let run_as: Vec<String> = run_as
        .iter()
        .map(|word| sh_single_quote(word))
        .collect();
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
    let run_as: Vec<String> = run_as
        .iter()
        .map(|word| format!(r#""{}""#, cmd_escape(word)))
        .collect();
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
    #[cfg(target_os = "wasi")]
    pnpm_fs::file_mode::set_path_permissions(path, 0o755)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
