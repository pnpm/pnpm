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
    let mut cold = Vec::with_capacity(args.runs);
    let mut warm = Vec::with_capacity(args.runs);
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
    for setting in [registry_setting.as_str(), "storeDir: ./store-dir", "cacheDir: ./cache-dir"] {
        add_setting(&mut settings, setting);
    }
    add_setting(&mut settings, "ignoreScripts: true");
    fs::write(&workspace_manifest, settings)
        .map_err(|error| format!("write {}: {error}", workspace_manifest.display()))
}

/// Append the top-level `key: value` line `setting` to the YAML in
/// `settings`, unless `settings` already sets that key.
fn add_setting(settings: &mut String, setting: &str) {
    let key = setting.split_once(':').map_or(setting, |(key, _)| key);
    let already_set = settings
        .lines()
        .any(|line| {
            line.strip_prefix(key)
                .is_some_and(|rest| rest.starts_with(':'))
        });
    if !already_set {
        if !settings.is_empty() && !settings.ends_with('\n') {
            settings.push('\n');
        }
        settings.push_str(setting);
        settings.push('\n');
    }
}

fn install(probe: &Path, project: &Path) -> Result<Duration, String> {
    let start = Instant::now();
    let status = Command::new(probe)
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
