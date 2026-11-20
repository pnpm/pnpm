//! Checks of the environment lifecycle scripts run in.
//!
//! pnpm is a native binary and starts without Node.js, but the `.bin` shims a
//! script calls look `node` up on `PATH`. A `PATH` that lost its Node.js, or a
//! shell that resolves it differently, only shows up once a script runs, as an
//! exit status of 127. These checks report what a script will see, and run one.

use super::{
    CheckResult,
    DoctorArgs,
    install_probe::last_message,
};
use crate::process::{
    Command,
    Output,
    Stdio,
};
use pnpm_config::Config;
use pnpm_executor::{
    VERIFY_DEPS_BEFORE_RUN_ENV,
    current_pnpm_exe,
    select_shell,
    use_shell_emulator,
};
use std::{
    collections::HashSet,
    env,
    ffi::OsStr,
    fmt::Write as _,
    fs,
    path::{
        Path,
        PathBuf,
    },
    time::Instant,
};

const NODE_FILE_NAME: &str = if cfg!(windows) { "node.exe" } else { "node" };

const NODE_FIX: &str = "Scripts that run a package's executable need Node.js on PATH. Put the directory of a working Node.js on PATH before running pnpm.";

/// The executable a probe script calls. It records the Node.js its shim
/// started it with in the file named by its argument.
const PROBE_BIN: &str = "pnpm-doctor-probe";
const PROBE_SCRIPT: &str =
    "#!/usr/bin/env node\nrequire('node:fs').writeFileSync(process.argv[2], process.execPath)\n";

/// The last lines of an `sh -x` trace worth reporting: the shim's lookup of
/// `node` and the `exec` it ends with.
#[cfg(unix)]
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
        .map(env::split_paths)
        .map(Iterator::collect)
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
            .map(|path| path.display())
            .map(|path| path.to_string())
            .collect();
        let _ = write!(detail, " (also on PATH: {})", rest.join(", "));
    }
    Some(detail)
}

fn find_node_entries(dirs: &[PathBuf]) -> (Vec<PathBuf>, Vec<UnusableNode>) {
    let mut usable = Vec::new();
    let mut unusable = Vec::new();
    let mut seen = HashSet::new();
    for candidate in dirs
        .iter()
        .map(|dir| dir.join(NODE_FILE_NAME))
    {
        if candidate.symlink_metadata().is_err() || !seen.insert(candidate.clone()) {
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

#[cfg(not(unix))]
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
        let shell = select_shell(script_shell, cfg!(windows)).map_err(|error| error.to_string());
        let resolved = shell.and_then(|shell| {
            which::which(&shell.program)
                .map_err(|_| format!("{} was not found", shell.program.display()))
        });
        match resolved {
            Ok(resolved) => describe_resolved(&resolved),
            Err(detail) => {
                return CheckResult::fail(
                    title,
                    detail,
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
    match which::which(program) {
        Ok(resolved) => describe_resolved(&resolved),
        Err(_) => format!("{} (not found)", program.display()),
    }
}

fn describe_resolved(resolved: &Path) -> String {
    match shell_behind(resolved) {
        Some(real) => format!("{} ({})", resolved.display(), real.display()),
        None => resolved.display().to_string(),
    }
}

/// The shell that `sh` stands for, outside Windows. macOS's `/bin/sh` starts whichever shell
/// `/private/var/select/sh` links to; elsewhere `sh` is usually a link.
fn shell_behind(sh: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    if cfg!(target_os = "macos") && sh == Path::new("/bin/sh") {
        return fs::read_link("/private/var/select/sh").ok();
    }
    let real = fs::canonicalize(sh).ok()?;
    (real != sh).then_some(real)
}

/// Install a project whose `postinstall` calls a dependency's executable, then
/// call the same executable through `pnpm exec` in `project_dir` when that is a
/// project. Both go through a generated shim that has to find `node`. The
/// install runs its script with the configured script shell.
pub(super) fn check_lifecycle_scripts(
    config: &Config,
    project_dir: &Path,
    args: &DoctorArgs,
) -> CheckResult {
    let benchmark = args.benchmark;
    let title = "Lifecycle scripts";
    let started = Instant::now();
    if which::which("node").is_err() {
        return CheckResult::pass(title, "skipped (no Node.js on PATH)").timed(benchmark, started);
    }
    let Ok(base) = tempfile::tempdir() else {
        return CheckResult::warn(
            title,
            "could not create a temporary directory",
            "Check that the system temp directory is writable.",
        )
        .timed(benchmark, started);
    };
    let probes = current_pnpm_exe()
        .map_err(|error| error.to_string())
        .and_then(|pnpm| {
            let consumer = write_probe_fixture(base.path()).map_err(|error| error.to_string())?;
            let probe = Probe {
                pnpm: &pnpm,
                base: base.path(),
                shim: consumer.join("node_modules/.bin").join(PROBE_BIN),
                shell_settings: shell_settings(config),
                offline: args.offline,
            };
            run_script_probes(&probe, &consumer, project_dir)
        });
    match probes {
        Ok(detail) => CheckResult::pass(title, detail),
        Err(detail) => CheckResult::fail(title, detail, NODE_FIX),
    }
    .timed(benchmark, started)
}

/// The `--config` flags that give the probe install the shell scripts run in
/// here. Its temporary project reads only the global config, and an empty
/// value cannot clear a global setting, so the effective shell is passed
/// even when it is the default.
fn shell_settings(config: &Config) -> Vec<String> {
    let script_shell = config.script_shell.as_deref().map(Path::new);
    let emulator = use_shell_emulator(config.shell_emulator, script_shell);
    let mut settings = vec![format!("--config.shell-emulator={emulator}")];
    if emulator {
        return settings;
    }
    // A configured shell goes as written, so one that scripts reject fails
    // the probe the same way.
    let shell = match script_shell.filter(|shell| !shell.as_os_str().is_empty()) {
        Some(shell) => Some(shell.to_path_buf()),
        None => select_shell(None, cfg!(windows)).ok().map(|shell| shell.program),
    };
    if let Some(shell) = shell {
        settings.push(format!("--config.script-shell={}", shell.display()));
    }
    settings
}

fn run_script_probes(
    probe: &Probe<'_>,
    consumer: &Path,
    project_dir: &Path,
) -> Result<String, String> {
    let node = probe
        .install_script(consumer)
        .map_err(|failure| probe.describe(failure, consumer))?;
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

/// The pnpm under test, the temporary directory the probes write to, the
/// shim of the probe executable, the script shell flags of the install, and
/// whether `pnpm doctor` runs with `--offline`.
struct Probe<'a> {
    pnpm: &'a Path,
    base: &'a Path,
    shim: PathBuf,
    shell_settings: Vec<String>,
    offline: bool,
}

impl Probe<'_> {
    fn install_script(&self, consumer: &Path) -> Result<String, String> {
        // Isolated from the real store and cache, a `CI=true` environment,
        // and any ancestor workspace, the same as the install smoke test.
        let output = Command::new(self.pnpm)
            .current_dir(consumer)
            .args([
                "install",
                "--offline",
                "--config.ignore-scripts=false",
                "--ignore-workspace",
                "--no-frozen-lockfile",
            ])
            .arg(format!("--store-dir={}", self.base.join("store").display()))
            .arg(format!("--cache-dir={}", self.base.join("cache").display()))
            .args(&self.shell_settings)
            .stdout(Stdio::null())
            .output();
        read_probe_result(output, &self.base.join("install-node"), "the install script")
    }

    fn project(&self, project_dir: &Path) -> Result<String, String> {
        let result = self.base.join("project-node");
        let output = self
            .exec_in(project_dir)
            .arg(&self.shim)
            .arg(&result)
            .stdout(Stdio::null())
            .output();
        read_probe_result(output, &result, &format!("a script in {}", project_dir.display()))
    }

    /// `pnpm exec` in `dir`, offline when `pnpm doctor` is, and kept from
    /// installing the project. The variable
    /// outranks every other source of `verifyDepsBeforeRun`, and the
    /// uppercase spelling is removed so it cannot take precedence.
    fn exec_in(&self, dir: &Path) -> Command {
        let mut command = Command::new(self.pnpm);
        command
            .current_dir(dir)
            .env_remove("PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN")
            .env(VERIFY_DEPS_BEFORE_RUN_ENV, "false");
        if self.offline {
            command.arg("--config.offline=true");
        }
        command.arg("exec");
        command
    }

    /// `failure` followed by an `sh -x` trace of the shim run in `dir`, which
    /// shows the `PATH` the shim searched and the command it ran.
    fn describe(&self, failure: String, dir: &Path) -> String {
        let mut detail = failure;
        for line in trace_shim(self, dir) {
            let _ = write!(detail, "\n    {line}");
        }
        detail
    }
}

#[cfg(unix)]
fn trace_shim(probe: &Probe<'_>, dir: &Path) -> Vec<String> {
    let Ok(output) = probe
        .exec_in(dir)
        .args(["/bin/sh", "-x"])
        .arg(&probe.shim)
        .arg(probe.base.join("trace-node"))
        .stdout(Stdio::null())
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
#[cfg(not(unix))]
fn trace_shim(_probe: &Probe<'_>, _dir: &Path) -> Vec<String> {
    Vec::new()
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
