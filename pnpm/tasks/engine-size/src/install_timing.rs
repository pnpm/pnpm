//! Wall-clock timings of the stripped probe installing a fixture.

use crate::cli_args::CliArgs;
use serde::Serialize;
use std::{
    ffi::OsString,
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

/// Directories that hold install output, not install input, at any depth.
const SKIPPED_DIRS: [&str; 2] = ["node_modules", ".git"];

/// The store and cache the benchmark creates in the project root.
const SKIPPED_ROOT_DIRS: [&str; 2] = ["store-dir", "cache-dir"];

#[derive(Debug, Serialize)]
pub struct InstallTimings {
    /// A frozen-lockfile install into an empty store, median milliseconds.
    pub cold_store_ms: u128,
    /// A frozen-lockfile install with the store populated and
    /// `node_modules` removed, median milliseconds.
    pub warm_store_ms: u128,
}

pub fn measure(probe: &Path, registry: &str, args: &CliArgs) -> Result<InstallTimings, String> {
    let fixture = args.fixture();
    let mut cold = Vec::with_capacity(args.runs.into());
    let mut warm = Vec::with_capacity(args.runs.into());
    for _ in 0..args.runs {
        let project = tempfile::tempdir().map_err(|error| format!("create a temp dir: {error}"))?;
        prepare_project(&fixture, project.path())?;
        let inherited = std::env::vars_os().map(|(name, _)| name);
        let env = benchmark_env(project.path(), registry, inherited);
        cold.push(install(probe, project.path(), &env)?);
        remove_dir(&project.path().join("node_modules"))?;
        warm.push(install(probe, project.path(), &env)?);
    }
    Ok(InstallTimings { cold_store_ms: median(cold), warm_store_ms: median(warm) })
}

/// Copy the fixture into `project` unchanged, with its local inputs
/// such as `file:` dependencies and patches, leaving out [`SKIPPED_DIRS`] and
/// [`SKIPPED_ROOT_DIRS`].
fn prepare_project(fixture: &Path, project: &Path) -> Result<(), String> {
    let fixture = fixture
        .canonicalize()
        .map_err(|error| format!("resolve {}: {error}", fixture.display()))?;
    let entries = walkdir::WalkDir::new(&fixture)
        .into_iter()
        .filter_entry(|entry| !is_install_output(entry));
    for entry in entries {
        let entry = entry.map_err(|error| format!("read {}: {error}", fixture.display()))?;
        let relative = entry
            .path()
            .strip_prefix(&fixture)
            .unwrap_or_else(|_| entry.path());
        let to = project.join(relative);
        let file_type = entry.file_type();
        let copied = if file_type.is_symlink() {
            copy_symlink(entry.path(), &to, &fixture)
        } else if file_type.is_dir() {
            fs::create_dir_all(&to)
        } else {
            fs::copy(entry.path(), &to).map(drop)
        };
        copied.map_err(|error| format!("copy {}: {error}", entry.path().display()))?;
    }
    Ok(())
}

/// Recreate the link at `from` as `to`. A target inside `fixture` keeps
/// its relative spelling, so it resolves inside the copy. A target
/// outside it is linked by its absolute path, which the copy can still
/// reach. A dangling link is recreated as it is.
fn copy_symlink(from: &Path, to: &Path, fixture: &Path) -> std::io::Result<()> {
    let raw_target = fs::read_link(from)?;
    let resolved = from
        .parent()
        .unwrap_or(fixture)
        .join(&raw_target)
        .canonicalize();
    let target = match resolved {
        Ok(resolved) if !resolved.starts_with(fixture) => resolved,
        Ok(_) => raw_target,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => raw_target,
        Err(error) => return Err(error),
    };
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, to);
    #[cfg(windows)]
    return if from.is_dir() {
        std::os::windows::fs::symlink_dir(target, to)
    } else {
        std::os::windows::fs::symlink_file(target, to)
    };
}

fn is_install_output(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name();
    let root_only = entry.depth() == 1
        && SKIPPED_ROOT_DIRS
            .iter()
            .any(|dir| name == *dir);
    entry.depth() > 0
        && (root_only
            || SKIPPED_DIRS
                .iter()
                .any(|dir| name == *dir))
}

/// One environment change for the probe: set `name` to the value, or
/// remove it when the value is `None`.
pub type EnvChange = (OsString, Option<OsString>);

/// The probe's environment changes. Every inherited pnpm or npm setting
/// is removed, so none can redirect the install out of `project`, except
/// the ones that decide how to reach the registry. The settings the benchmark
/// owns, including the paths the install writes to, are then set as
/// `PNPM_CONFIG_*` variables, which take precedence over the fixture's
/// files.
///
/// `lockfileDir` is left to the fixture: setting it, even to `project`,
/// makes a frozen install fetch several times more package metadata and
/// run many times slower.
pub fn benchmark_env(
    project: &Path,
    registry: &str,
    inherited: impl IntoIterator<Item = OsString>,
) -> Vec<EnvChange> {
    let settings: [(&str, OsString); 6] = [
        ("PNPM_CONFIG_REGISTRY", registry.into()),
        ("PNPM_CONFIG_STORE_DIR", project.join("store-dir").into_os_string()),
        ("PNPM_CONFIG_CACHE_DIR", project.join("cache-dir").into_os_string()),
        ("PNPM_CONFIG_IGNORE_SCRIPTS", "true".into()),
        ("PNPM_CONFIG_MODULES_DIR", project.join("node_modules").into_os_string()),
        ("PNPM_CONFIG_VIRTUAL_STORE_DIR", project.join("node_modules/.pnpm").into_os_string()),
    ];
    let removals = inherited
        .into_iter()
        .filter(|name| config_setting(name).is_some_and(|setting| !reaches_registry(&setting)))
        .map(|name| (name, None));
    let assignments = settings
        .into_iter()
        .map(|(name, value)| (OsString::from(name), Some(value)));
    removals.chain(assignments).collect()
}

/// The lowercased setting a `PNPM_CONFIG_*` or `NPM_CONFIG_*` variable
/// sets, in either case.
fn config_setting(name: &std::ffi::OsStr) -> Option<String> {
    let name = name.to_str()?.to_ascii_lowercase();
    ["pnpm_config_", "npm_config_"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .map(str::to_string)
}

/// Settings that only decide how to reach the registry, which may need
/// a proxy, a custom CA, or credentials.
const REGISTRY_ACCESS_SETTINGS: [&str; 12] = [
    "userconfig",
    "npmrc_auth_file",
    "proxy",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "noproxy",
    "strict_ssl",
    "ca",
    "cafile",
    "cert",
    "key",
];

/// URL-scoped settings (`//host/:_authToken`), top-level auth such as
/// `_auth`, and [`REGISTRY_ACCESS_SETTINGS`].
fn reaches_registry(setting: &str) -> bool {
    let setting = setting.replace('-', "_");
    setting.starts_with("//")
        || setting.starts_with('_')
        || REGISTRY_ACCESS_SETTINGS.contains(&setting.as_str())
}

fn install(probe: &Path, project: &Path, env: &[EnvChange]) -> Result<Duration, String> {
    let mut command = Command::new(probe);
    for (name, value) in env {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    let start = Instant::now();
    let status = command
        .arg(project)
        .arg("--frozen-lockfile")
        .status()
        .map_err(|error| format!("run {}: {error}", probe.display()))?;
    let elapsed = start.elapsed();
    if status.success() {
        Ok(elapsed)
    } else {
        Err(format!("the probe failed to install {}: {status}", project.display()))
    }
}

fn remove_dir(dir: &Path) -> Result<(), String> {
    fs::remove_dir_all(dir).map_err(|error| format!("remove {}: {error}", dir.display()))
}

fn median(mut durations: Vec<Duration>) -> u128 {
    durations.sort();
    durations
        .get(durations.len() / 2)
        .map_or(0, Duration::as_millis)
}

#[cfg(test)]
mod tests;
