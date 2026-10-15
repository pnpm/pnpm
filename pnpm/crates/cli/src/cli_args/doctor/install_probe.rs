//! The offline install the release gate relies on, and reading the reason out
//! of a failed pnpm run's error report.

use super::CheckResult;
use std::{fs, path::Path, process::Command, time::Instant};

/// Install a throwaway package as a `file:` dependency, entirely offline, to
/// confirm this binary can resolve, fetch into the store, and link a dependency
/// end to end. Catches both classes of broken release the release gate exists
/// for: a binary that will not run at all, and one whose install path crashes.
pub(super) fn check_install_smoke_test(benchmark: bool) -> CheckResult {
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
    let pnpm = pnpm_executor::current_pnpm_exe().map_err(|error| error.to_string())?;
    let output = Command::new(pnpm)
        .current_dir(&consumer)
        .args(["install", "--offline", "--ignore-scripts"])
        .arg(format!("--store-dir={}", store.display()))
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = last_message(&stderr);
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

/// The last message of a pnpm error report, with the lines the report wrapped
/// it onto joined back together.
pub(super) fn last_message(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let start = lines
        .iter()
        .rposition(|line| starts_message(line))
        .unwrap_or(0);
    let words: Vec<&str> = lines[start..]
        .iter()
        .map(|line| line.trim())
        .collect();
    words
        .join(" ")
        .trim_start_matches(MESSAGE_MARKERS)
        .trim_start()
        .to_owned()
}

const MESSAGE_MARKERS: &[char] = &['×', '╰', '├', '─', '▶'];

fn starts_message(line: &str) -> bool {
    !line.starts_with(char::is_whitespace) || line.trim_start().starts_with(MESSAGE_MARKERS)
}
