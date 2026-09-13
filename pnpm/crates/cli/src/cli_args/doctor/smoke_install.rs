use super::{CheckResult, Command, Instant, Path, fs};

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

pub(super) fn run_install_smoke_test(base: &Path) -> Result<(), String> {
    let provider = base.join("provider");
    let consumer = base.join("consumer");
    let store = base.join("store");
    fs::create_dir_all(&provider).map_err(|error| error.to_string())?;
    fs::create_dir_all(&consumer).map_err(|error| error.to_string())?;
    fs::write(
        provider.join("package.json"),
        r#"{"name":"pnpm-doctor-fixture","version":"0.0.0"}"#,
    )
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

    check_smoke_install_output(&output)?;
    if !consumer.join("node_modules/pnpm-doctor-fixture/package.json").exists() {
        return Err("install reported success but the dependency was not linked".to_owned());
    }
    Ok(())
}

pub(super) fn last_line(text: &str) -> String {
    text
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_owned()
}

pub(super) fn check_smoke_install_output(output: &std::process::Output) -> Result<(), String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = last_line(stderr.trim());
        return Err(format!(
            r#"offline "file:" install failed{}"#,
            if reason.is_empty() {
                String::new()
            } else {
                format!(": {reason}")
            },
        ));
    }
    Ok(())
}
