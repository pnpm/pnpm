//! Checks of the environment lifecycle scripts run in.
//!
//! pnpm is a native binary and starts without Node.js, but the `.bin` shims a
//! script calls look `node` up on `PATH`. A `PATH` that lost its Node.js, or a
//! shell that resolves it differently, only shows up once a script runs, as an
//! exit status of 127. These checks report what a script will see, and run one.

use super::{CheckResult, install_probe::last_message};
use pnpm_config::Config;
use pnpm_executor::{current_pnpm_exe, select_shell, use_shell_emulator};
use std::{
    env,
    ffi::OsStr,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Instant,
};

const NODE_FILE_NAME: &str = if cfg!(windows) { "node.exe" } else { "node" };

const NODE_FIX: &str = "Scripts that run a package's executable need Node.js on PATH. Put the directory of a working Node.js on PATH before running pnpm.";

/// The executable a probe script calls. It records the Node.js its shim
/// started it with in the file named by its argument.
const PROBE_BIN: &str = "pnpm-doctor-probe";
const PROBE_SCRIPT: &str =
    "#!/usr/bin/env node\nrequire('node:fs').writeFileSync(process.argv[2], process.execPath)\n";

/// Keeps a `pnpm exec` probe from installing the project it runs in.
const NO_VERIFY_DEPS: &str = "--config.verify-deps-before-run=false";

/// The last lines of an `sh -x` trace worth reporting: the shim's lookup of
/// `node` and the `exec` it ends with.
const TRACE_LINES: usize = 20;

/// A `node` entry of a `PATH` directory that a lookup skips, and why.
#[derive(Debug, PartialEq, Eq)]
struct UnusableNode {
    path: PathBuf,
    reason: &'static str,
}

/// List the `node` executables on `path` in lookup order, the first one being
/// the one scripts run, and warn about entries a lookup skips, such as a
/// broken link left behind by a version manager.
pub(super) fn check_node_on_path(path: Option<&OsStr>) -> CheckResult {
    let title = "Node.js on PATH";
    let dirs: Vec<PathBuf> = path
        .map(|path| env::split_paths(path).collect())
        .unwrap_or_default();
    let (usable, unusable) = find_node_entries(&dirs);
    let found = describe_usable_nodes(&usable);
    if unusable.is_empty() {
        return match found {
            Some(found) => CheckResult::pass(title, found),
            None => CheckResult::warn(title, "no node executable found", NODE_FIX),
        };
    }
    let skipped: Vec<String> = unusable
        .iter()
        .map(|entry| format!("{} ({})", entry.path.display(), entry.reason))
        .collect();
    let found = found.unwrap_or_else(|| "no usable node executable".to_owned());
    CheckResult::warn(
        title,
        format!("{found}; skipped: {}", skipped.join(", ")),
        "Repair or remove the skipped entries. A version manager that relinks node while scripts run can make them fail with exit status 127.",
    )
}

fn describe_usable_nodes(usable: &[PathBuf]) -> Option<String> {
    let (first, rest) = usable.split_first()?;
    let mut detail = first.display().to_string();
    if !rest.is_empty() {
        let rest: Vec<String> = rest
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        let _ = write!(detail, " (also on PATH: {})", rest.join(", "));
    }
    Some(detail)
}

fn find_node_entries(dirs: &[PathBuf]) -> (Vec<PathBuf>, Vec<UnusableNode>) {
    let mut usable = Vec::new();
    let mut unusable = Vec::new();
    for candidate in dirs
        .iter()
        .map(|dir| dir.join(NODE_FILE_NAME))
    {
        if candidate.symlink_metadata().is_err() {
            continue;
        }
        match unusable_reason(&candidate) {
            None => usable.push(candidate),
            Some(reason) => unusable.push(UnusableNode { path: candidate, reason }),
        }
    }
    (usable, unusable)
}

fn unusable_reason(candidate: &Path) -> Option<&'static str> {
    let Ok(metadata) = fs::metadata(candidate) else {
        return Some("broken link");
    };
    if !metadata.is_file() {
        return Some("not a file");
    }
    (!is_executable(&metadata)).then_some("not executable")
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(windows)]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

/// Report the shell that runs scripts and, outside Windows, the `/bin/sh` that
/// starts every `.bin` shim, since the two can differ.
pub(super) fn check_script_shell(config: &Config) -> CheckResult {
    let title = "Script shell";
    let script_shell = config.script_shell.as_deref().map(Path::new);
    let scripts = if use_shell_emulator(config.shell_emulator, script_shell) {
        "the built-in shell emulator".to_owned()
    } else {
        match select_shell(script_shell, cfg!(windows)) {
            Ok(shell) => describe_program(&shell.program),
            Err(error) => {
                return CheckResult::fail(
                    title,
                    error.to_string(),
                    "Point scriptShell at a shell that can run scripts.",
                );
            }
        }
    };
    if cfg!(windows) {
        return CheckResult::pass(title, format!("scripts run in {scripts}"));
    }
    let shims = describe_program(Path::new("/bin/sh"));
    CheckResult::pass(title, format!("scripts run in {scripts}, package executables in {shims}"))
}

fn describe_program(program: &Path) -> String {
    let Ok(resolved) = which::which(program) else {
        return format!("{} (not found)", program.display());
    };
    match shell_behind(&resolved) {
        Some(real) => format!("{} ({})", resolved.display(), real.display()),
        None => resolved.display().to_string(),
    }
}

/// The shell that `sh` stands for. macOS's `/bin/sh` starts whichever shell
/// `/private/var/select/sh` links to; elsewhere `sh` is usually a link.
fn shell_behind(sh: &Path) -> Option<PathBuf> {
    if cfg!(target_os = "macos") && sh == Path::new("/bin/sh") {
        return fs::read_link("/private/var/select/sh").ok();
    }
    let real = fs::canonicalize(sh).ok()?;
    (real != sh).then_some(real)
}

/// Install a project whose `postinstall` calls a dependency's executable, then
/// call the same executable through `pnpm exec` in `project_dir` when that is a
/// project. Both go through a generated shim that has to find `node`.
pub(super) fn check_lifecycle_scripts(project_dir: &Path, benchmark: bool) -> CheckResult {
    let title = "Lifecycle scripts";
    if which::which("node").is_err() {
        return CheckResult::pass(title, "skipped (no Node.js on PATH)");
    }
    let started = Instant::now();
    let Ok(base) = tempfile::tempdir() else {
        return CheckResult::warn(
            title,
            "could not create a temporary directory",
            "Check that the system temp directory is writable.",
        );
    };
    let probes = current_pnpm_exe()
        .map_err(|error| error.to_string())
        .and_then(|pnpm| run_script_probes(&pnpm, base.path(), project_dir));
    match probes {
        Ok(detail) => CheckResult::pass(title, detail).timed(benchmark, started),
        Err(detail) => CheckResult::fail(title, detail, NODE_FIX),
    }
}

fn run_script_probes(pnpm: &Path, base: &Path, project_dir: &Path) -> Result<String, String> {
    let consumer = write_probe_fixture(base).map_err(|error| error.to_string())?;
    let probe = Probe { pnpm, base, shim: consumer.join("node_modules/.bin").join(PROBE_BIN) };
    let node = probe
        .install_script(&consumer)
        .map_err(|failure| probe.describe(failure, &consumer))?;
    let mut detail = format!("an install script ran {node}");
    if !project_dir.join("package.json").is_file() {
        return Ok(detail);
    }
    let node = probe
        .project(project_dir)
        .map_err(|failure| probe.describe(failure, project_dir))?;
    let _ = write!(detail, ", a script in {} ran {node}", project_dir.display());
    Ok(detail)
}

/// Write a `file:` dependency with the probe executable and a project whose
/// `postinstall` calls it, and return the project's directory.
fn write_probe_fixture(base: &Path) -> std::io::Result<PathBuf> {
    let provider = base.join("provider");
    let consumer = base.join("consumer");
    fs::create_dir_all(&provider)?;
    fs::create_dir_all(&consumer)?;
    let provider_manifest = serde_json::json!({
        "name": PROBE_BIN,
        "version": "0.0.0",
        "bin": { PROBE_BIN: "probe.js" },
    });
    let consumer_manifest = serde_json::json!({
        "name": "pnpm-doctor-scripts",
        "version": "0.0.0",
        "private": true,
        "dependencies": { PROBE_BIN: "file:../provider" },
        "scripts": { "postinstall": format!("{PROBE_BIN} ../install-node") },
    });
    fs::write(provider.join("package.json"), provider_manifest.to_string())?;
    fs::write(provider.join("probe.js"), PROBE_SCRIPT)?;
    fs::write(consumer.join("package.json"), consumer_manifest.to_string())?;
    Ok(consumer)
}

/// The pnpm under test, the temporary directory the probes write to, and the
/// shim of the probe executable.
struct Probe<'a> {
    pnpm: &'a Path,
    base: &'a Path,
    shim: PathBuf,
}

impl Probe<'_> {
    fn install_script(&self, consumer: &Path) -> Result<String, String> {
        let output = Command::new(self.pnpm)
            .current_dir(consumer)
            .args(["install", "--offline", "--config.ignore-scripts=false"])
            .arg(format!("--store-dir={}", self.base.join("store").display()))
            .output();
        read_probe_result(output, &self.base.join("install-node"), "the install script")
    }

    fn project(&self, project_dir: &Path) -> Result<String, String> {
        let result = self.base.join("project-node");
        let output = Command::new(self.pnpm)
            .current_dir(project_dir)
            .args([NO_VERIFY_DEPS, "exec"])
            .arg(&self.shim)
            .arg(&result)
            .output();
        read_probe_result(output, &result, &format!("a script in {}", project_dir.display()))
    }

    /// `failure` followed by an `sh -x` trace of the shim run in `dir`, which
    /// shows the `PATH` the shim searched and where `node` went missing.
    fn describe(&self, failure: String, dir: &Path) -> String {
        let mut detail = failure;
        for line in self.trace_shim(dir) {
            let _ = write!(detail, "\n    {line}");
        }
        detail
    }

    #[cfg(unix)]
    fn trace_shim(&self, dir: &Path) -> Vec<String> {
        let Ok(output) = Command::new(self.pnpm)
            .current_dir(dir)
            .args([NO_VERIFY_DEPS, "exec", "/bin/sh", "-x"])
            .arg(&self.shim)
            .arg(self.base.join("trace-node"))
            .output()
        else {
            return Vec::new();
        };
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = stderr.lines().collect();
        let start = lines.len().saturating_sub(TRACE_LINES);
        lines[start..]
            .iter()
            .map(|line| (*line).to_owned())
            .collect()
    }

    /// Windows runs the `.cmd` shim, which has no trace to show.
    #[cfg(windows)]
    fn trace_shim(&self, _dir: &Path) -> Vec<String> {
        Vec::new()
    }
}

fn read_probe_result(
    output: std::io::Result<Output>,
    result: &Path,
    what: &str,
) -> Result<String, String> {
    let output = output.map_err(|error| format!("{what} could not start: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{what} failed: {}", last_message(&stderr)));
    }
    fs::read_to_string(result).map_err(|_| format!("{what} did not run the probe executable"))
}

#[cfg(test)]
mod tests;
