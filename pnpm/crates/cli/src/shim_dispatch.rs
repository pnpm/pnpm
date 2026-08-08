//! The dispatcher behind context-aware global shims.
//!
//! Every bin pnpm links into the global bin dir is a shim that invokes
//! `pnpm --shim <name> <global-target> -- <args...>` (see
//! `pacquet_cmd_shim::ShimStyle`). The dispatcher walks up from the
//! current directory looking for a project that provides the same bin —
//! either an installed `node_modules/.bin/<name>` entry or, for the
//! managed runtimes, a `devEngines.runtime` / `engines.runtime` version
//! pin — and runs the project's version instead of the global target, so
//! bare `node` (or `tsc`, `eslint`, ...) follows the project the user is
//! standing in.
//!
//! Running a project's binaries just because the user typed a command
//! inside its directory is a trust decision, gated twice. First, the
//! candidate must be provided by the same-named package as the global
//! bin — the switch only ever substitutes a different version of what
//! the user already installed globally. Second, the project must either
//! carry a lockfile this machine's own install gate verified (see
//! `lockfile_verified_on_this_machine`), or be approved on the terminal
//! (`Do you trust this project?`), with answers persisted in a
//! machine-local registry — both signals deliberately live outside the
//! project, so a cloned repository cannot ship its own approval.
//! Without a terminal the dispatcher falls back to the global target.

use pacquet_config::{
    Host, WorkspaceSettings, default_cache_dir, default_config_dir, default_pnpm_home_dir,
    default_state_dir,
};
use pacquet_fs::lexical_normalize;
use pacquet_lockfile_verification::lockfile_verified_on_this_machine;
use pacquet_package_manifest::is_runtime_alias;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::Command,
};

/// Environment variable that short-circuits the dispatcher to the global
/// target: a user-facing kill switch, and the recursion guard for the
/// children the dispatcher itself spawns.
const BYPASS_ENV: &str = "PNPM_SHIM_BYPASS";

/// Test-only escape hatch mirroring `PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS`:
/// treats every project as trusted without prompting or recording.
const AUTO_TRUST_ENV: &str = "PNPM_AUTO_APPROVE_PROJECT_BINS_FOR_TESTS";

/// The trust registry, one JSON record per line, last record per
/// project wins: `{"projectDir": <abs path>, "allow": bool,
/// "decidedAt": <ms since epoch>}`. Appends of one line are atomic on
/// POSIX and NTFS, so concurrent shim invocations need no coordination
/// (same design as the lockfile-verified cache, and the same wire
/// format the TypeScript CLI reads and writes).
const TRUST_FILE_NAME: &str = "global-bin-trust.jsonl";

/// Intercept a `pnpm --shim ...` invocation. `None` means argv is not a
/// shim dispatch and the regular CLI should proceed; `Some(code)` means
/// the dispatch ran (or failed) and the process must exit with `code`.
/// On Unix a successful dispatch never returns at all — the target is
/// `exec`ed in place.
pub(crate) fn try_dispatch(argv: &[OsString]) -> Option<i32> {
    if argv.get(1).and_then(|arg| arg.to_str()) != Some("--shim") {
        return None;
    }
    Some(dispatch(&argv[2..]))
}

fn dispatch(rest: &[OsString]) -> i32 {
    let Some((name, global_target, args)) = parse_shim_argv(rest) else {
        eprintln!(
            "pnpm: malformed --shim invocation. Usage: pnpm --shim <name> <target> -- [args...]",
        );
        return 1;
    };
    if bypass_requested() || !global_shims_enabled() {
        return run_global_target(global_target, args);
    }
    let candidate = std::env::current_dir()
        .ok()
        .and_then(|cwd| find_candidate(&cwd, name))
        .filter(|candidate| candidate_matches_global_provider(candidate, global_target, name));
    match candidate {
        Some(candidate) if is_trusted(&candidate, name) => match candidate {
            Candidate::LocalBin { bin, .. } => exec_program(&bin, args),
            Candidate::RuntimePin { version_spec, .. } => {
                run_runtime_via_dlx(name, &version_spec, args)
            }
        },
        _ => run_global_target(global_target, args),
    }
}

/// Run the embedded global target the way a direct shim would: a script
/// target goes through its shebang/extension interpreter (resolved next
/// to the dispatcher binary — the global bin dir — before `PATH`), a
/// binary target is executed as-is.
fn run_global_target(target: &Path, args: &[OsString]) -> i32 {
    let runtime = pacquet_cmd_shim::search_script_runtime::<pacquet_cmd_shim::Host>(target);
    let Ok(Some(runtime)) = runtime else {
        return exec_program(target, args);
    };
    let Some(prog) = &runtime.prog else {
        return exec_program(target, args);
    };
    let mut interpreter_args: Vec<OsString> =
        runtime.args.split_whitespace().map(OsString::from).collect();
    interpreter_args.push(target.as_os_str().to_owned());
    interpreter_args.extend(args.iter().cloned());
    exec_program(&resolve_sibling_program(prog), &interpreter_args)
}

/// Resolve an interpreter name the way the shim scripts do: prefer the
/// program sitting next to the dispatcher binary (`$basedir/<prog>`),
/// fall back to a `PATH` lookup by name.
fn resolve_sibling_program(prog: &str) -> PathBuf {
    if let Ok(own_exe) = std::env::current_exe()
        && let Some(bin_dir) = own_exe.parent()
    {
        let file_names: Vec<String> = if cfg!(windows) {
            vec![format!("{prog}.exe"), prog.to_string()]
        } else {
            vec![prog.to_string()]
        };
        for file_name in file_names {
            let candidate = bin_dir.join(file_name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(prog)
}

/// Split the machine-generated tail of a `--shim` invocation:
/// `<name> <target> -- [args...]`.
fn parse_shim_argv(rest: &[OsString]) -> Option<(&str, &Path, &[OsString])> {
    let [name, target, separator, args @ ..] = rest else {
        return None;
    };
    if separator.to_str() != Some("--") {
        return None;
    }
    Some((name.to_str()?, Path::new(target), args))
}

fn bypass_requested() -> bool {
    std::env::var(BYPASS_ENV)
        .is_ok_and(|value| !value.is_empty() && value != "0" && value != "false")
}

/// The `globalShims` setting at dispatch time, so turning it off takes
/// effect immediately instead of waiting for the next global install to
/// relink the shims. Read from the env override and the global
/// `config.yaml` only — the sources a project cannot influence.
fn global_shims_enabled() -> bool {
    for env_name in ["PNPM_CONFIG_GLOBAL_SHIMS", "pnpm_config_global_shims"] {
        if let Ok(value) = std::env::var(env_name)
            && !value.is_empty()
            && let Ok(enabled) = serde_json::from_str::<bool>(&value)
        {
            return enabled;
        }
    }
    default_config_dir::<Host>()
        .and_then(|config_dir| WorkspaceSettings::load_global(&config_dir).ok().flatten())
        .and_then(|settings| settings.global_shims)
        .unwrap_or(true)
}

/// The context switch only ever substitutes a different version of the
/// same package the user installed globally: the local candidate must be
/// provided by the same-named package as the embedded global target.
/// A project shipping a same-named bin from a *different* package (a
/// lookalike `tsc` from `evil-pkg`) fails the match and the global
/// version runs.
fn candidate_matches_global_provider(
    candidate: &Candidate,
    global_target: &Path,
    name: &str,
) -> bool {
    let Some(global_provider) = provider_package_of_path(global_target) else {
        return false;
    };
    match candidate {
        Candidate::LocalBin { bin, .. } => {
            local_bin_provider(bin, name).as_deref() == Some(global_provider.as_str())
        }
        // A runtime pin entry already carries the runtime's name, so the
        // match reduces to "the global bin is that runtime's own binary".
        Candidate::RuntimePin { .. } => global_provider == name,
    }
}

/// The package a path under some `node_modules` belongs to: the one or
/// two (`@scope/name`) components after the last `node_modules` segment.
fn provider_package_of_path(path: &Path) -> Option<String> {
    let normalized = lexical_normalize(path);
    let components: Vec<&str> = normalized
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();
    let modules_index = components.iter().rposition(|part| *part == "node_modules")?;
    let first = components.get(modules_index + 1)?;
    if first.starts_with('@') {
        let second = components.get(modules_index + 2)?;
        Some(format!("{first}/{second}"))
    } else {
        Some((*first).to_string())
    }
}

/// The package providing a `node_modules/.bin` entry. A symlinked bin
/// (the runtime binaries) resolves through its link target; a cmd-shim
/// script is attributed via the `# cmd-shim-target=` trailer of its
/// extensionless flavor (`name`, not `name.cmd` — only the sh flavor
/// carries the trailer). `None` — e.g. a Windows `node.exe` hardlink —
/// counts as no match.
fn local_bin_provider(bin: &Path, name: &str) -> Option<String> {
    let metadata = std::fs::symlink_metadata(bin).ok()?;
    let target = if metadata.file_type().is_symlink() {
        std::fs::read_link(bin).ok()?
    } else {
        read_shim_target(&bin.parent()?.join(name))?
    };
    let resolved = if target.is_absolute() { target } else { bin.parent()?.join(target) };
    provider_package_of_path(&resolved)
}

/// The target recorded in a cmd-shim script's trailing
/// `# cmd-shim-target=` marker.
fn read_shim_target(script: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(script).ok()?;
    content
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("# cmd-shim-target="))
        .map(PathBuf::from)
}

/// A project-provided replacement for the invoked global bin.
enum Candidate {
    /// The project has `node_modules/.bin/<name>` — an installed
    /// dependency (including a materialized runtime) providing the bin.
    LocalBin { project_dir: PathBuf, bin: PathBuf },
    /// The project pins the runtime `<name>` in `devEngines.runtime` /
    /// `engines.runtime` but has not materialized it; the pinned version
    /// is fetched into the store on demand.
    RuntimePin { project_dir: PathBuf, version_spec: String },
}

impl Candidate {
    fn project_dir(&self) -> &Path {
        match self {
            Candidate::LocalBin { project_dir, .. } | Candidate::RuntimePin { project_dir, .. } => {
                project_dir
            }
        }
    }
}

/// Walk up from `cwd` to the nearest directory providing `name`. An
/// installed `.bin` entry and a manifest runtime pin are looked for at
/// every level so the nearest provider of either kind wins; within one
/// directory the installed bin wins over the pin. Directories inside the
/// pnpm home are skipped — pnpm's own global installs are not projects.
fn find_candidate(cwd: &Path, name: &str) -> Option<Candidate> {
    let pnpm_home = default_pnpm_home_dir::<Host>();
    let runtime = is_runtime_alias(name);
    for dir in cwd.ancestors() {
        if pnpm_home.as_deref().is_some_and(|home| dir.starts_with(home)) {
            continue;
        }
        if let Some(bin) = local_bin_path(dir, name) {
            return Some(Candidate::LocalBin { project_dir: dir.to_path_buf(), bin });
        }
        if runtime && let Some(version_spec) = manifest_runtime_pin(dir, name) {
            return Some(Candidate::RuntimePin { project_dir: dir.to_path_buf(), version_spec });
        }
    }
    None
}

/// The runnable `node_modules/.bin` entry for `name` under `dir`, if any.
/// A broken symlink does not count — `is_file` follows links.
fn local_bin_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let bin_dir = dir.join("node_modules").join(".bin");
    let candidates: Vec<String> = if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd"), name.to_string()]
    } else {
        vec![name.to_string()]
    };
    candidates.iter().map(|file_name| bin_dir.join(file_name)).find(|path| path.is_file())
}

/// The version a project's `package.json` pins for the runtime `name`,
/// `devEngines.runtime` first, then `engines.runtime` — the same
/// precedence as the pre-command runtime check. The pin counts whatever
/// its `onFail` policy says: the dispatcher only chooses which version to
/// run, it does not modify the project.
fn manifest_runtime_pin(dir: &Path, name: &str) -> Option<String> {
    let bytes = std::fs::read(dir.join("package.json")).ok()?;
    let manifest: Value = serde_json::from_slice(&bytes).ok()?;
    for engines_field in ["devEngines", "engines"] {
        let Some(runtime) = manifest.get(engines_field).and_then(|field| field.get("runtime"))
        else {
            continue;
        };
        let entries = match runtime {
            Value::Array(entries) => entries.iter().collect::<Vec<_>>(),
            single => vec![single],
        };
        for entry in entries {
            if entry.get("name").and_then(Value::as_str) == Some(name)
                && let Some(version) = entry.get("version").and_then(Value::as_str)
            {
                let version = version.trim();
                if !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }
    }
    None
}

fn is_trusted(candidate: &Candidate, name: &str) -> bool {
    if std::env::var(AUTO_TRUST_ENV).as_deref() == Ok("1") {
        return true;
    }
    let project_dir = candidate.project_dir();
    // An installed bin from a lockfile that this machine's own install
    // gate verified needs no prompt: the user (or a tool they ran)
    // already installed these exact dependencies here through pnpm's
    // integrity + tarball-URL binding checks. The cache record lives
    // outside the repository, so a cloned project cannot ship one. The
    // download-on-demand path stays behind the prompt — its mirror
    // configuration is repo-controlled.
    if matches!(candidate, Candidate::LocalBin { .. }) && lockfile_verified_locally(project_dir) {
        return true;
    }
    let project_key = lexical_normalize(project_dir).display().to_string();
    let trust_file = default_state_dir::<Host>().map(|dir| dir.join(TRUST_FILE_NAME));
    if let Some(trust_file) = &trust_file
        && let Some(allow) = read_trust_decision(trust_file, &project_key)
    {
        return allow;
    }
    let Some(allow) = prompt_for_trust(&project_key, name) else {
        return false;
    };
    if let Some(trust_file) = &trust_file {
        // Best-effort: an unwritable state dir means re-prompting next
        // time, which is strictly better than failing the command.
        let _ = append_trust_decision(trust_file, &project_key, allow);
    }
    allow
}

/// Whether this machine's install gate verified the project's lockfile
/// as it currently sits on disk.
fn lockfile_verified_locally(project_dir: &Path) -> bool {
    let cache_dir = default_cache_dir::<Host>();
    lockfile_verified_on_this_machine(&cache_dir, &project_dir.join("pnpm-lock.yaml"))
}

/// The recorded decision for `project_key`, last record wins.
fn read_trust_decision(trust_file: &Path, project_key: &str) -> Option<bool> {
    let content = std::fs::read_to_string(trust_file).ok()?;
    let mut decision = None;
    for line in content.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if record.get("projectDir").and_then(Value::as_str) == Some(project_key) {
            decision = record.get("allow").and_then(Value::as_bool);
        }
    }
    decision
}

fn append_trust_decision(trust_file: &Path, project_key: &str, allow: bool) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = trust_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let decided_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    let record = json!({ "projectDir": project_key, "allow": allow, "decidedAt": decided_at });
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(trust_file)?;
    writeln!(file, "{record}")
}

/// Ask on the terminal whether the project's bins may be run. `None`
/// means the question could not be answered (CI, no TTY, or the prompt
/// was interrupted) — the caller falls back to the global target and
/// records nothing, so the next interactive invocation asks again.
fn prompt_for_trust(project_key: &str, name: &str) -> Option<bool> {
    if is_ci::cached() || !std::io::stdin().is_terminal() {
        return None;
    }
    let prompt = format!(
        "The project at \"{project_key}\" provides its own \"{name}\", which will be used instead of the globally installed one.\nDo you trust this project?",
    );
    dialoguer::Confirm::new().with_prompt(prompt).default(false).interact().ok()
}

/// Materialize the pinned runtime through `pnpm dlx <name>@runtime:<spec>`
/// and run it with `args`. dlx already solves everything this needs —
/// resolving the version, fetching into the store, caching the
/// materialized slot, and forwarding stdio and the exit code.
fn run_runtime_via_dlx(name: &str, version_spec: &str, args: &[OsString]) -> i32 {
    let Ok(own_exe) = std::env::current_exe() else {
        eprintln!("pnpm: cannot locate the pnpm binary to fetch {name}@{version_spec}");
        return 1;
    };
    let status = Command::new(own_exe)
        .arg("dlx")
        .arg(format!("{name}@runtime:{version_spec}"))
        .args(args)
        .env(BYPASS_ENV, "1")
        .status();
    match status {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("pnpm: failed to run {name}@runtime:{version_spec} via dlx: {error}");
            1
        }
    }
}

/// Run `program` with `args`, replacing this process where the platform
/// allows. Exit codes follow the shell convention: 127 when the program
/// does not exist, 126 when it cannot be executed.
#[cfg(unix)]
fn exec_program(program: &Path, args: &[OsString]) -> i32 {
    use std::os::unix::process::CommandExt as _;
    let error = Command::new(program).args(args).exec();
    eprintln!("pnpm: failed to exec {}: {error}", program.display());
    if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
}

#[cfg(windows)]
fn exec_program(program: &Path, args: &[OsString]) -> i32 {
    // `.cmd`/`.bat` targets are scripts for the command interpreter,
    // not executables — route them through `cmd /c`.
    let extension = program.extension().and_then(|ext| ext.to_str()).unwrap_or_default();
    let mut command =
        if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat") {
            let mut command = Command::new("cmd");
            command.arg("/c").arg(program);
            command
        } else {
            Command::new(program)
        };
    match command.args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("pnpm: failed to run {}: {error}", program.display());
            if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
        }
    }
}

#[cfg(test)]
mod tests;
