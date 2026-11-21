//! `pacquet doctor`: diagnose the pnpm installation and the environment it
//! runs in.
//!
//! The checks are the ones that predict whether an install will work on this
//! machine and how fast it will be, plus one live check, an offline `file:`
//! install, that drives the resolve/store/link path end to end. The release
//! pipeline runs this same command against a freshly published version before
//! moving its dist-tags, so what gates a release is what ships to users.

mod filesystem;
mod install_method;
mod install_probe;
mod probe_path;
mod scripts;

use crate::{
    cli_args::ping::PingArgs,
    process::Command,
};
use clap::Args;
use filesystem::check_filesystem_capabilities;
use install_method::check_install_method;
use pnpm_config::{
    Config,
    PNPM_VERSION,
};
use pnpm_fs::Host;
use probe_path::{
    can_write_to_dir,
    check_writable_dir,
    dir_is_in_path,
    prefetch_config_paths,
};
use serde::Serialize;
use std::{
    fmt::Write as _,
    path::{
        Path,
        PathBuf,
    },
    time::Instant,
};

const DOCTOR_FETCH_TIMEOUT: u64 = 15_000;

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Skip checks that need network access.
    #[clap(long)]
    pub offline: bool,

    /// Also time filesystem and install operations.
    #[clap(long)]
    pub benchmark: bool,

    /// Report the results as JSON.
    #[clap(long)]
    pub json: bool,
}

/// Whether every check passed. The caller turns `Unhealthy` into a non-zero
/// exit; see `dispatch_query::doctor`.
#[derive(Debug, PartialEq, Eq)]
pub enum DoctorOutcome {
    Healthy,
    Unhealthy,
}

/// What a check concluded. `Warn` reports something worth fixing that does not
/// stop pnpm from working, so it does not fail the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckResult {
    title: String,
    status: CheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// A concrete next step, shown when the check does not pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u128>,
}

impl CheckResult {
    fn pass(title: &str, detail: impl Into<String>) -> Self {
        CheckResult {
            title: title.to_owned(),
            status: CheckStatus::Pass,
            detail: Some(detail.into()),
            fix: None,
            duration_ms: None,
        }
    }

    fn warn(title: &str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        CheckResult {
            title: title.to_owned(),
            status: CheckStatus::Warn,
            detail: Some(detail.into()),
            fix: Some(fix.into()),
            duration_ms: None,
        }
    }

    fn fail(title: &str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        CheckResult {
            title: title.to_owned(),
            status: CheckStatus::Fail,
            detail: Some(detail.into()),
            fix: Some(fix.into()),
            duration_ms: None,
        }
    }

    fn timed(mut self, benchmark: bool, started: Instant) -> Self {
        if benchmark {
            self.duration_ms = Some(started.elapsed().as_millis());
        }
        self
    }
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    checks: Vec<CheckResult>,
}

/// The report to print and whether it should fail the command.
pub struct DoctorResult {
    pub output: String,
    pub outcome: DoctorOutcome,
}

impl DoctorArgs {
    /// Run every check and render the report. Returns the text (or JSON) to
    /// print alongside the outcome, leaving printing and the exit status to
    /// the caller.
    ///
    /// `project_dir` is where a script probe runs when it holds a project.
    pub async fn run(&self, config: &Config, project_dir: &Path) -> miette::Result<DoctorResult> {
        prefetch_config_paths(config);
        let mut checks = vec![check_versions(), check_install_method(config)];
        checks.push(scripts::check_node_on_path(std::env::var_os("PATH").as_deref()));
        checks.push(scripts::check_script_shell(config, project_dir));
        checks.push(check_global_bin_dir(config));
        checks.push(check_writable_dir("Cache directory", &config.cache_dir));
        checks.push(check_writable_dir("Store directory", config.store_dir.root()));
        let fs_started = Instant::now();
        checks.push(
            check_filesystem_capabilities::<Host>(config).timed(self.benchmark, fs_started),
        );
        checks.push(self.check_connectivity(config).await);
        checks.push(install_probe::check_install_smoke_test(self.benchmark));
        checks.push(scripts::check_lifecycle_scripts(config, project_dir, self));

        let outcome = if checks
            .iter()
            .any(|check| check.status == CheckStatus::Fail)
        {
            DoctorOutcome::Unhealthy
        } else {
            DoctorOutcome::Healthy
        };

        let report = DoctorReport { checks };
        let output = if self.json {
            serde_json::to_string_pretty(&report)
                .map_err(|error| {
                    miette::miette!("Failed to render the doctor report as JSON: {error}")
                })?
        } else {
            render_report(&report)
        };
        Ok(DoctorResult { output, outcome })
    }

    async fn check_connectivity(&self, config: &Config) -> CheckResult {
        let title = "Registry connectivity";
        if self.offline {
            return CheckResult::pass(title, "skipped (--offline)");
        }
        let started = Instant::now();
        match (PingArgs { registry: None }).run(&connectivity_config(config)).await {
            Ok(_) => CheckResult::pass(
                title,
                format!("{} ({}ms)", config.registry, started.elapsed().as_millis()),
            ),
            Err(error) => CheckResult::fail(
                title,
                format!("could not reach {}: {error}", config.registry),
                "Check your network, proxy, and registry configuration.",
            ),
        }
    }
}

fn connectivity_config(config: &Config) -> Config {
    Config { fetch_timeout: DOCTOR_FETCH_TIMEOUT, ..config.clone() }
}

fn check_versions() -> CheckResult {
    let detail = match node_version() {
        Some(node_version) => format!("pnpm {PNPM_VERSION}, Node.js {node_version}"),
        None => format!("pnpm {PNPM_VERSION}"),
    };
    CheckResult::pass("Versions", detail)
}

/// Report the Node.js that lifecycle scripts will run under. pacquet is a
/// native binary, so Node is not required for pnpm itself to work; its
/// absence is worth reporting, not failing on.
fn node_version() -> Option<String> {
    let output = Command::new("node")
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?;
    Some(
        version
            .trim()
            .trim_start_matches('v')
            .to_owned(),
    )
}

/// Check the global executables directory, where the CLI links binaries and
/// which must be on `PATH` for them to run. The layout moved between majors
/// (v10 links into `PNPM_HOME` directly, v11 into `PNPM_HOME/bin`), so accept
/// whichever candidate `PATH` actually contains.
fn check_global_bin_dir(config: &Config) -> CheckResult {
    let title = "Global bin directory";
    let candidates: Vec<PathBuf> = [config.global_bin_dir.clone(), config.global_dir.clone()]
        .into_iter()
        .flatten()
        .collect();
    let Some(first) = candidates.first() else {
        return CheckResult::pass(title, "not configured");
    };

    let Some(path_var) = std::env::var_os("PATH") else {
        return CheckResult::warn(
            title,
            "the PATH environment variable is not set",
            r#"Run "pnpm setup" to add it to your shell configuration."#,
        );
    };
    let path_dirs: Vec<PathBuf> = pnpm_fs::split_paths(&path_var).collect();

    let Some(bin_dir) = candidates.iter().find(|dir| dir_is_in_path(dir, &path_dirs)) else {
        return CheckResult::warn(
            title,
            format!("{} is not in PATH", first.display()),
            r#"Run "pnpm setup" to add it to your shell configuration."#,
        );
    };
    if !can_write_to_dir(bin_dir) {
        return CheckResult::fail(
            title,
            format!("no write access to {}", bin_dir.display()),
            r#"Run "pnpm setup", or fix the directory permissions."#,
        );
    }
    CheckResult::pass(title, bin_dir.display().to_string())
}

fn render_report(report: &DoctorReport) -> String {
    let mut lines: Vec<String> = report.checks
        .iter()
        .map(|check| {
            let mut line = format!("{} {}", status_mark(check.status), check.title);
            if let Some(detail) = &check.detail {
                let _ = write!(line, ": {detail}");
            }
            if let Some(duration) = check.duration_ms {
                let _ = write!(line, " ({duration}ms)");
            }
            if check.status != CheckStatus::Pass
                && let Some(fix) = &check.fix
            {
                let _ = write!(line, "\n    {fix}");
            }
            line
        })
        .collect();

    let failed = report.checks
        .iter()
        .filter(|check| check.status == CheckStatus::Fail)
        .count();
    let warned = report.checks
        .iter()
        .filter(|check| check.status == CheckStatus::Warn)
        .count();
    let summary = if failed > 0 {
        format!("{failed} check(s) failed")
    } else if warned > 0 {
        format!("All checks passed with {warned} warning(s)")
    } else {
        "All checks passed".to_owned()
    };
    lines.push(String::new());
    lines.push(summary);
    lines.join("\n")
}

fn status_mark(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Pass => "✓",
        CheckStatus::Warn => "‼",
        CheckStatus::Fail => "✗",
    }
}

#[cfg(test)]
mod tests;
