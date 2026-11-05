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
        prepare_project(&fixture, project.path())?;
        let env = benchmark_env(project.path(), registry);
        cold.push(install(probe, project.path(), &env)?);
        remove_dir(&project.path().join("node_modules"))?;
        warm.push(install(probe, project.path(), &env)?);
    }
    Ok(InstallTimings { cold_store_ms: median(cold), warm_store_ms: median(warm) })
}

/// Copy the fixture's files into `project` unchanged.
fn prepare_project(fixture: &Path, project: &Path) -> Result<(), String> {
    for file in FIXTURE_FILES {
        let from = fixture.join(file);
        if from.exists() {
            fs::copy(&from, project.join(file))
                .map_err(|error| format!("copy {}: {error}", from.display()))?;
        }
    }
    Ok(())
}

/// One environment change for the probe: set `name` to the value, or
/// remove it when the value is `None`.
pub type EnvChange = (String, Option<OsString>);

/// The settings the benchmark owns, passed as `PNPM_CONFIG_*` variables,
/// which take precedence over the fixture's files. The lowercase spelling
/// of each is removed first, so an inherited one cannot compete. Every
/// other variable, credentials included, is inherited unchanged.
pub fn benchmark_env(project: &Path, registry: &str) -> Vec<EnvChange> {
    let settings: [(&str, OsString); 4] = [
        ("REGISTRY", registry.into()),
        ("STORE_DIR", project.join("store-dir").into_os_string()),
        ("CACHE_DIR", project.join("cache-dir").into_os_string()),
        ("IGNORE_SCRIPTS", "true".into()),
    ];
    let removals = settings
        .iter()
        .map(|(suffix, _)| (format!("pnpm_config_{}", suffix.to_lowercase()), None));
    let assignments = settings
        .iter()
        .map(|(suffix, value)| (format!("PNPM_CONFIG_{suffix}"), Some(value.clone())));
    removals.chain(assignments).collect()
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
