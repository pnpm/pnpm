//! `pacquet doctor` — diagnose the pnpm installation and the environment it
//! runs in.
//!
//! The checks are the ones that predict whether an install will work on this
//! machine and how fast it will be, plus one live check — an offline `file:`
//! install — that drives the resolve/store/link path end to end. The release
//! pipeline runs this same command against a freshly published version before
//! moving its dist-tags, so what gates a release is what ships to users.

use crate::cli_args::ping::PingArgs;
use clap::Args;
use pacquet_config::Config;
use serde::Serialize;
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

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
    pub async fn run(&self, config: &Config) -> miette::Result<DoctorResult> {
        let mut checks = vec![check_versions(), check_install_method()];
        checks.push(check_global_bin_dir(config));
        checks.push(check_writable_dir("Cache directory", &config.cache_dir));
        checks.push(check_writable_dir("Store directory", config.store_dir.root()));
        checks.push(check_filesystem_capabilities(config, self.benchmark));
        checks.push(self.check_connectivity(config).await);
        checks.push(check_install_smoke_test(self.benchmark));

        let outcome = if checks.iter().any(|check| check.status == CheckStatus::Fail) {
            DoctorOutcome::Unhealthy
        } else {
            DoctorOutcome::Healthy
        };

        let report = DoctorReport { checks };
        let output = if self.json {
            serde_json::to_string_pretty(&report).map_err(|error| {
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
        match (PingArgs { registry: None }).run(config).await {
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

fn check_versions() -> CheckResult {
    let pnpm_version = env!("CARGO_PKG_VERSION");
    let detail = match node_version() {
        Some(node_version) => format!("pnpm {pnpm_version}, Node.js {node_version}"),
        None => format!("pnpm {pnpm_version}"),
    };
    CheckResult::pass("Versions", detail)
}

/// Report the Node.js that lifecycle scripts will run under. pacquet is a
/// native binary, so Node is not required for pnpm itself to work — its
/// absence is worth reporting, not failing on.
fn node_version() -> Option<String> {
    let output = Command::new("node").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?;
    Some(version.trim().trim_start_matches('v').to_owned())
}

fn check_install_method() -> CheckResult {
    let title = "Install method";
    if std::env::var_os("COREPACK_ROOT").is_some() {
        return CheckResult::warn(
            title,
            "pnpm, run by Corepack",
            r#"Corepack manages the pnpm version itself; "pnpm self-update" is unavailable under it."#,
        );
    }
    CheckResult::pass(title, "pnpm")
}

/// Check the global executables directory — where the CLI links binaries and
/// which must be on `PATH` for them to run. The layout moved between majors
/// (v10 links into `PNPM_HOME` directly, v11 into `PNPM_HOME/bin`), so accept
/// whichever candidate `PATH` actually contains.
fn check_global_bin_dir(config: &Config) -> CheckResult {
    let title = "Global bin directory";
    let candidates: Vec<PathBuf> =
        [config.global_bin_dir.clone(), config.global_dir.clone()].into_iter().flatten().collect();
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
    let path_dirs: Vec<PathBuf> = std::env::split_paths(&path_var).collect();

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

fn dir_is_in_path(dir: &Path, path_dirs: &[PathBuf]) -> bool {
    let canonical = dir.canonicalize();
    path_dirs.iter().any(|entry| {
        entry == dir
            || match (&canonical, entry.canonicalize()) {
                (Ok(dir), Ok(entry)) => dir == &entry,
                _ => false,
            }
    })
}

fn check_writable_dir(title: &str, dir: &Path) -> CheckResult {
    if !can_write_to_dir(dir) {
        return CheckResult::fail(
            title,
            format!("no write access to {}", dir.display()),
            "Fix the directory permissions or point the setting at a writable path.",
        );
    }
    CheckResult::pass(title, dir.display().to_string())
}

/// Probe which link strategies work from the store's volume, since that is what
/// determines how packages land in `node_modules` and how fast an install is: a
/// reflink (copy-on-write) or hardlink is near-free, a plain copy is not.
fn check_filesystem_capabilities(config: &Config, benchmark: bool) -> CheckResult {
    let title = "Filesystem";
    let started = Instant::now();
    let probe_dir = tempfile::tempdir_in(config.store_dir.root()).or_else(|_| tempfile::tempdir());
    let Ok(probe_dir) = probe_dir else {
        return CheckResult::warn(
            title,
            "could not create a probe directory",
            "Check that the store directory and the system temp directory are writable.",
        );
    };

    let Ok(capabilities) = probe_link_capabilities(probe_dir.path()) else {
        return CheckResult::warn(
            title,
            "could not write a probe file",
            "Check that the store directory is writable.",
        );
    };

    let available: Vec<&str> =
        capabilities.iter().filter(|(_, supported)| *supported).map(|(name, _)| *name).collect();
    let has_cheap_link = capabilities
        .iter()
        .any(|(name, supported)| *supported && matches!(*name, "reflink" | "hardlink"));

    let result = if has_cheap_link {
        CheckResult::pass(title, format!("available: {}", available.join(", ")))
    } else {
        CheckResult::warn(
            title,
            "only copying is available",
            "Neither reflink nor hardlink works between the store and this project; installs will copy files and be slower. Put the store on the same filesystem as your projects.",
        )
    };
    result.timed(benchmark, started)
}

fn probe_link_capabilities(dir: &Path) -> std::io::Result<[(&'static str, bool); 3]> {
    let source = dir.join("source");
    fs::write(&source, "pnpm-doctor")?;
    Ok([
        ("reflink", reflink_copy::reflink(&source, dir.join("reflink")).is_ok()),
        ("hardlink", fs::hard_link(&source, dir.join("hardlink")).is_ok()),
        ("symlink", symlink_file(&source, &dir.join("symlink")).is_ok()),
    ])
}

#[cfg(unix)]
fn symlink_file(source: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, link)
}

#[cfg(windows)]
fn symlink_file(source: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(source, link)
}

/// Install a throwaway package as a `file:` dependency, entirely offline, to
/// confirm this binary can resolve, fetch into the store, and link a dependency
/// end to end. Catches both classes of broken release the release gate exists
/// for: a binary that will not run at all, and one whose install path crashes.
fn check_install_smoke_test(benchmark: bool) -> CheckResult {
    let title = "Install smoke test";
    let started = Instant::now();
    let Ok(base) = tempfile::tempdir() else {
        return CheckResult::warn(
            title,
            "could not create a temporary directory",
            "Check that the system temp directory is writable.",
        );
    };
    match run_install_smoke_test(base.path()) {
        Ok(()) => CheckResult::pass(title, r#"offline "file:" install linked its dependency"#)
            .timed(benchmark, started),
        Err(detail) => CheckResult::fail(
            title,
            detail,
            r#"Run "pnpm install" in a scratch project to see the full error."#,
        ),
    }
}

fn run_install_smoke_test(base: &Path) -> Result<(), String> {
    let provider = base.join("provider");
    let consumer = base.join("consumer");
    let store = base.join("store");
    fs::create_dir_all(&provider).map_err(|error| error.to_string())?;
    fs::create_dir_all(&consumer).map_err(|error| error.to_string())?;
    fs::write(provider.join("package.json"), r#"{"name":"pnpm-doctor-fixture","version":"0.0.0"}"#)
        .map_err(|error| error.to_string())?;
    fs::write(
        consumer.join("package.json"),
        r#"{"name":"pnpm-doctor-consumer","version":"0.0.0","private":true,"dependencies":{"pnpm-doctor-fixture":"file:../provider"}}"#,
    )
    .map_err(|error| error.to_string())?;

    // A throwaway store keeps the probe from writing into the real one. The
    // fixture is a temp directory with no lockfile and no workspace above it,
    // so nothing here depends on the lockfile or workspace flags.
    let current_exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(current_exe)
        .current_dir(&consumer)
        .args(["install", "--offline", "--ignore-scripts"])
        .arg(format!("--store-dir={}", store.display()))
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = last_line(stderr.trim());
        return Err(format!(
            r#"offline "file:" install failed{}"#,
            if reason.is_empty() { String::new() } else { format!(": {reason}") },
        ));
    }
    if !consumer.join("node_modules/pnpm-doctor-fixture/package.json").exists() {
        return Err("install reported success but the dependency was not linked".to_owned());
    }
    Ok(())
}

fn last_line(text: &str) -> String {
    text.lines().rfind(|line| !line.trim().is_empty()).unwrap_or_default().to_owned()
}

fn can_write_to_dir(dir: &Path) -> bool {
    let probe = dir.join(format!(".pnpm-doctor-write-{}", std::process::id()));
    let written = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    written
}

fn render_report(report: &DoctorReport) -> String {
    let mut lines: Vec<String> = report
        .checks
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

    let failed = report.checks.iter().filter(|check| check.status == CheckStatus::Fail).count();
    let warned = report.checks.iter().filter(|check| check.status == CheckStatus::Warn).count();
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
