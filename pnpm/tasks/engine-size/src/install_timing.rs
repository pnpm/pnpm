//! Wall-clock timings of the stripped probe installing a fixture.

use crate::cli_args::CliArgs;
use serde::Serialize;
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

/// The files of a fixture copied into each install's project directory.
const FIXTURE_FILES: [&str; 3] = ["package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml"];

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
        prepare_project(&fixture, project.path(), registry)?;
        cold.push(install(probe, project.path())?);
        remove_dir(&project.path().join("node_modules"))?;
        warm.push(install(probe, project.path())?);
    }
    Ok(InstallTimings { cold_store_ms: median(cold), warm_store_ms: median(warm) })
}

/// Copy the fixture into `project` and point it at `registry`, with a
/// store and cache inside `project`.
fn prepare_project(fixture: &Path, project: &Path, registry: &str) -> Result<(), String> {
    for file in FIXTURE_FILES {
        let from = fixture.join(file);
        if from.exists() {
            fs::copy(&from, project.join(file))
                .map_err(|error| format!("copy {}: {error}", from.display()))?;
        }
    }
    let workspace_manifest = project.join("pnpm-workspace.yaml");
    let mut settings = fs::read_to_string(&workspace_manifest).unwrap_or_default();
    let registry_setting = format!("registry: {registry}");
    for setting in [
        registry_setting.as_str(),
        "storeDir: ./store-dir",
        "cacheDir: ./cache-dir",
        "ignoreScripts: true",
    ] {
        set_setting(&mut settings, setting);
    }
    fs::write(&workspace_manifest, settings)
        .map_err(|error| format!("write {}: {error}", workspace_manifest.display()))
}

/// Set the top-level `key: value` line `setting` in the YAML in
/// `settings`, replacing the key's existing entry and its indented or
/// block-sequence continuation lines.
pub fn set_setting(settings: &mut String, setting: &str) {
    let key = setting.split_once(':').map_or(setting, |(key, _)| key);
    let mut kept = String::with_capacity(settings.len() + setting.len() + 1);
    let mut in_replaced_entry = false;
    for line in settings.lines() {
        let continues_entry = line.starts_with([' ', '\t', '-']);
        if line
            .strip_prefix(key)
            .is_some_and(|rest| rest.starts_with(':'))
        {
            in_replaced_entry = true;
        } else if !(in_replaced_entry && continues_entry) {
            in_replaced_entry = false;
            kept.push_str(line);
            kept.push('\n');
        }
    }
    kept.push_str(setting);
    kept.push('\n');
    *settings = kept;
}

/// Run the probe on `project` without the `PNPM_CONFIG_*` and
/// `npm_config_*` variables of the caller's environment, which would
/// override the settings [`prepare_project`] writes.
fn install(probe: &Path, project: &Path) -> Result<Duration, String> {
    let start = Instant::now();
    let status = Command::new(probe)
        .env_clear()
        .envs(std::env::vars_os().filter(|(name, _)| !is_config_variable(name)))
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

fn is_config_variable(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_lowercase();
    name.starts_with("pnpm_config_") || name.starts_with("npm_config_")
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
