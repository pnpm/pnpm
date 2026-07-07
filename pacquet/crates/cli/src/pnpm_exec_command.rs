//! The `pnpmExecCommand` re-exec step.
//!
//! When `pnpm-workspace.yaml` sets `pnpmExecCommand`, the project has
//! delegated pnpm-binary selection to an external command: pnpm runs it
//! once per user invocation, treats its trimmed stdout as the absolute
//! path of the pnpm binary the project must run under, and re-executes
//! into that binary when it differs from the running one. This runs
//! before command dispatch — mirroring pnpm's `main()`, which resolves
//! and re-execs before its `packageManager` handling — so the whole
//! command, including the `devEngines.packageManager` validation, runs
//! inside the binary the command selected.
//!
//! Behavioral contract shared with the TypeScript CLI
//! (`pnpm11/pnpm/src/pnpmExecCommand.ts`): the error codes and
//! messages, the `PNPM_EXEC_PATH` sentinel, the `PNPM_RE_EXEC_DEPTH`
//! backstop, and the trust-on-first-use notice recorded in
//! `pnpm-state.json`.

use crate::cli_args::{CliArgs, CliCommand, config::ConfigSubcommand};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::{Host, WorkspaceSettings, default_state_dir};
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Sentinel set on the child of a `pnpmExecCommand` re-exec. It carries
/// the resolved binary path so that the child (and any nested pnpm
/// invocation that inherits the environment, e.g. from a lifecycle
/// script) skips re-running the command: the resolution is done once
/// per user invocation.
pub(crate) const PNPM_EXEC_PATH_ENV: &str = "PNPM_EXEC_PATH";

/// Sentinel env var carrying the re-exec depth. A correct setup
/// terminates on the first re-exec (the child inherits
/// [`PNPM_EXEC_PATH_ENV`] and never spawns again), but a misconfigured
/// command could keep redirecting; this is the hard backstop against
/// fork-bombing.
const RE_EXEC_DEPTH_ENV: &str = "PNPM_RE_EXEC_DEPTH";
const MAX_RE_EXEC_DEPTH: u32 = 2;

const COMMAND_TIMEOUT: Duration = Duration::from_mins(1);

/// Errors raised by the `pnpmExecCommand` flow. Codes and messages
/// match the TypeScript CLI's `PnpmError`s (the `ERR_PNPM_` prefix is
/// part of the public contract).
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PnpmExecCommandError {
    #[display(
        r#"The pnpmExecCommand setting must be an array of non-empty strings, e.g. ["my-tool", "which-pnpm"]"#
    )]
    #[diagnostic(code(ERR_PNPM_EXEC_COMMAND_INVALID))]
    Invalid,

    #[display(r#"The pnpmExecCommand ("{command}") failed{status_suffix}"#)]
    #[diagnostic(code(ERR_PNPM_EXEC_COMMAND_FAIL))]
    Fail {
        command: String,
        /// `" with exit code N"` when the command exited, empty when it
        /// could not be spawned or timed out.
        status_suffix: String,
        #[help]
        hint: Option<String>,
    },

    #[display(r#"The pnpmExecCommand ("{command}") printed no path to stdout"#)]
    #[diagnostic(code(ERR_PNPM_EXEC_COMMAND_NO_OUTPUT))]
    NoOutput { command: String },

    #[display(r#"The pnpmExecCommand ("{command}") printed a non-absolute path: "{path}""#)]
    #[diagnostic(code(ERR_PNPM_EXEC_COMMAND_RELATIVE_PATH))]
    RelativePath { command: String, path: String },

    #[display(
        r#"The pnpmExecCommand ("{command}") printed a path that is not an existing file: "{path}""#
    )]
    #[diagnostic(code(ERR_PNPM_EXEC_COMMAND_BAD_PATH))]
    BadPath { command: String, path: String },

    #[display(
        r#"Failed to switch pnpm to {target}. Looks like pnpm CLI is missing at "{bin_dir}" or is incorrect"#
    )]
    #[diagnostic(code(ERR_PNPM_VERSION_SWITCH_FAIL))]
    VersionSwitchFail {
        target: String,
        bin_dir: String,
        #[help]
        hint: Option<String>,
    },
}

/// Resolve `pnpmExecCommand` for the parsed invocation and re-exec into
/// the binary it names when that isn't the running one (in which case
/// this function exits the process with the child's status and never
/// returns).
///
/// A no-op when the subcommand opts out of package-manager handling
/// (mirroring pnpm's `skipPackageManagerCheck` set), when `--global` is
/// used, when no workspace declares the setting, or when a parent pnpm
/// already resolved it (the [`PNPM_EXEC_PATH_ENV`] sentinel). Workspace
/// discovery or yaml errors are also a no-op here: command dispatch
/// hits the same error moments later and owns its reporting.
pub(crate) fn resolve_and_re_exec(args: &CliArgs) -> miette::Result<()> {
    if skips_binary_selection(&args.command) || is_global(&args.command) {
        return Ok(());
    }
    let Ok(dir) = dunce::canonicalize(&args.dir) else {
        return Ok(());
    };
    let Ok(Some(workspace_dir)) = pacquet_workspace::find_workspace_dir(&dir) else {
        return Ok(());
    };
    let Ok(Some(settings)) = WorkspaceSettings::load_exact(&workspace_dir) else {
        return Ok(());
    };
    let Some(value) = settings.pnpm_exec_command else {
        return Ok(());
    };
    apply(&value, &workspace_dir).map_err(miette::Report::new)
}

/// The command set that skips `pnpmExecCommand` handling, mirroring
/// pnpm's `shouldSkipPmHandling`: the commands with
/// `skipPackageManagerCheck = true` plus `setup`. (`--version` and
/// `help` never reach this point — clap short-circuits them during
/// parsing, as pnpm's arg parser does.)
fn skips_binary_selection(command: &CliCommand) -> bool {
    matches!(
        command,
        CliCommand::Setup(_)
            | CliCommand::Completion(_)
            | CliCommand::CompletionServer(_)
            | CliCommand::Dlx(_)
            | CliCommand::With(_)
            | CliCommand::SelfUpdate(_)
            | CliCommand::Runtime(_)
            | CliCommand::FindHash(_)
            | CliCommand::CatIndex(_)
            | CliCommand::CatFile(_)
            | CliCommand::Store(_)
    )
}

/// Whether the invocation carries `--global`, which opts out of
/// project-level binary pinning (matching pnpm, where `--global` skips
/// the whole package-manager block).
fn is_global(command: &CliCommand) -> bool {
    match command {
        CliCommand::Add(args) => args.global,
        CliCommand::ApproveBuilds(args) => args.global,
        CliCommand::Bin(args) => args.global,
        CliCommand::Config(args) => match &args.command {
            ConfigSubcommand::Set(args) => args.flags.global,
            ConfigSubcommand::Get(args) => args.flags.global,
            ConfigSubcommand::Delete(args) => args.flags.global,
            ConfigSubcommand::List(args) => args.flags.global,
        },
        CliCommand::List(args) | CliCommand::Ll(args) => args.global,
        CliCommand::Outdated(args) => args.global,
        CliCommand::Prefix(args) => args.global,
        CliCommand::Remove(args) => args.global,
        CliCommand::Root(args) => args.global,
        CliCommand::Update(args) => args.global,
        _ => false,
    }
}

/// Validate the raw yaml `value`, run the command, and re-exec into the
/// resolved binary. See [`resolve_and_re_exec`] for the flow; split out
/// so it operates on the raw setting plus the workspace dir only.
fn apply(value: &serde_json::Value, workspace_dir: &Path) -> Result<(), PnpmExecCommandError> {
    let command = validate(value)?;

    if std::env::var_os(PNPM_EXEC_PATH_ENV).is_some() {
        // A parent pnpm already ran the command and re-exec'd into its
        // result. If that result is the running binary (the normal
        // case), there is nothing to do. If it isn't — e.g. a stale
        // sentinel inherited from an unrelated parent process —
        // re-running the command could loop, so proceed and let the
        // packageManager check surface any version mismatch.
        return Ok(());
    }

    let trust = TrustRecord::check(&command, workspace_dir);

    let bin_path = run_command(&command)?;
    if let Some(trust) = trust {
        eprintln!("Resolved to {}", bin_path.display());
        // Record the command only after it resolved successfully, so a
        // failing first run doesn't silence the notice on the next
        // (successful) one.
        trust.persist();
    }

    if is_current_binary(&bin_path) {
        // Mark resolution as done for nested pnpm invocations.
        //
        // SAFETY: this runs in `main` before the tokio runtime and
        // rayon pool start, so the process is single-threaded and no
        // other thread can be reading the environment concurrently.
        unsafe {
            std::env::set_var(PNPM_EXEC_PATH_ENV, &bin_path);
        }
        return Ok(());
    }

    re_exec(&bin_path, &command)
}

fn validate(value: &serde_json::Value) -> Result<Vec<String>, PnpmExecCommandError> {
    let items = value.as_array().ok_or(PnpmExecCommandError::Invalid)?;
    if items.is_empty() {
        return Err(PnpmExecCommandError::Invalid);
    }
    items
        .iter()
        .map(|item| match item.as_str() {
            Some(arg) if !arg.is_empty() => Ok(arg.to_string()),
            _ => Err(PnpmExecCommandError::Invalid),
        })
        .collect()
}

/// Trust-on-first-use record of `pnpmExecCommand` values in
/// `pnpm-state.json`, keyed by the real path of the workspace
/// directory. A workspace whose command matches its record runs
/// silently; an unseen workspace or a changed command prints a notice
/// to stderr first — the same pattern as SSH known hosts, turning a
/// quietly edited `pnpm-workspace.yaml` into a visible signal. Stored
/// in the per-user state dir, outside the repository, so a project
/// cannot pre-seed it to suppress its own notice.
struct TrustRecord {
    /// `None` when the trust store couldn't be consulted safely (the
    /// notice was printed anyway); [`persist`](Self::persist) is then a
    /// no-op and the notice repeats next run.
    store: Option<TrustStore>,
}

struct TrustStore {
    state_file: PathBuf,
    state: serde_json::Map<String, serde_json::Value>,
    workspace_key: String,
    command_record: String,
}

impl TrustRecord {
    /// Print a notice to stderr when the workspace's command is unseen
    /// or changed, returning the record for the caller to
    /// [`persist`](Self::persist) once resolution succeeds. Returns
    /// `None` when the command matches its record (no notice printed).
    ///
    /// stderr keeps stdout machine-clean (`$(pnpm --version)` etc.).
    /// When the trust store can't be consulted safely — no resolvable
    /// state dir, a relative one, or an unreadable state file — the
    /// notice prints on every run and nothing is recorded: failing open
    /// on noise, never on silence.
    fn check(command: &[String], workspace_dir: &Path) -> Option<Self> {
        let workspace_key = dunce::canonicalize(workspace_dir)
            .unwrap_or_else(|_| workspace_dir.to_path_buf())
            .display()
            .to_string();
        let command_record =
            serde_json::to_string(command).expect("serialize a Vec<String> to JSON");
        // The trust records deliberately live in the *default* per-user
        // state dir, not the configured `stateDir`: that setting is
        // workspace-yaml-settable, so honoring it here would let the
        // workspace file that declares a malicious command also point
        // pnpm at a repo-controlled state file that pre-seeds its own
        // trust record, suppressing the notice. The env override
        // (`pnpm_config_state_dir`) is user-controlled, not
        // repo-controlled, so it stays honored.
        let Some(state_dir) = std::env::var("pnpm_config_state_dir")
            .or_else(|_| std::env::var("PNPM_CONFIG_STATE_DIR"))
            .map(PathBuf::from)
            .ok()
            .or_else(default_state_dir::<Host>)
        else {
            print_first_use_notice(command);
            return Some(TrustRecord { store: None });
        };
        // A relative state dir would resolve against the current
        // (typically repo-controlled) directory, so the trust record
        // could be pre-seeded. Reachable only with a relative env
        // override.
        if !state_dir.is_absolute() {
            print_first_use_notice(command);
            return Some(TrustRecord { store: None });
        }
        let state_file = state_dir.join("pnpm-state.json");

        let state = match fs::read_to_string(&state_file) {
            // An unparsable file is rewritten by `persist` (nothing
            // valid is lost).
            Ok(text) => serde_json::from_str::<serde_json::Map<_, _>>(&text).unwrap_or_default(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                serde_json::Map::default()
            }
            // Any other read failure (e.g. permissions) leaves a file
            // whose other keys `persist` would clobber, so skip
            // persistence for this run.
            Err(_) => {
                print_first_use_notice(command);
                return Some(TrustRecord { store: None });
            }
        };

        let seen = state
            .get("pnpmExecCommands")
            .and_then(|records| records.get(&workspace_key))
            .and_then(serde_json::Value::as_str);
        match seen {
            Some(seen) if seen == command_record => return None,
            Some(seen) => {
                let was = serde_json::from_str::<Vec<String>>(seen).map_or_else(
                    // A corrupted record still gets shown (escaped)
                    // rather than crashing the notice that is reporting
                    // the change away from it.
                    |_| escape_control_characters(seen),
                    |args| display_command(&args),
                );
                eprintln!("The pnpmExecCommand for this workspace has changed:");
                eprintln!("  was: {was}");
                eprintln!("  now: {}", display_command(command));
            }
            None => print_first_use_notice(command),
        }

        Some(TrustRecord {
            store: Some(TrustStore { state_file, state, workspace_key, command_record }),
        })
    }

    /// Record the command as seen. Failures are ignored: the notice
    /// then repeats next run, and noise is an acceptable failure mode
    /// where a suppressed notice is not.
    fn persist(self) {
        let Some(mut store) = self.store else {
            return;
        };
        if let Some(records) = store
            .state
            .entry("pnpmExecCommands")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
            .as_object_mut()
        {
            records.insert(store.workspace_key, serde_json::Value::String(store.command_record));
        }
        if let Some(parent) = store.state_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        // Tab-indented plus a trailing newline: the format
        // `write-json-file` produces, so the two CLIs rewrite the
        // shared `pnpm-state.json` identically.
        let mut text = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
        let mut serializer = serde_json::Serializer::with_formatter(&mut text, formatter);
        if serde::Serialize::serialize(&store.state, &mut serializer).is_ok() {
            text.push(b'\n');
            let _ = fs::write(&store.state_file, text);
        }
    }
}

fn print_first_use_notice(command: &[String]) {
    eprintln!("Resolving the pnpm binary with pnpmExecCommand:");
    eprintln!("> {}", display_command(command));
}

fn display_command(command: &[String]) -> String {
    escape_control_characters(&command.join(" "))
}

/// The notice is a trust signal, so argv elements must not be able to
/// forge it (or hide parts of it) with embedded newlines or terminal
/// escape sequences. Control characters are rendered as their JSON
/// escape. Kept in sync with the TypeScript CLI's
/// `escapeControlCharacters`.
fn escape_control_characters(text: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            ch if ch.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(ch));
            }
            ch => out.push(ch),
        }
    }
    out
}

/// Run the resolver command with stdout piped and stderr inherited (so
/// the tool's own diagnostics reach the user directly), returning the
/// absolute path of the binary it printed. Any failure — non-zero
/// exit, no output, a non-absolute or non-existent path — is a hard
/// error: the project delegated binary selection to the command, so
/// running the current (potentially mismatched) pnpm instead would
/// defeat the point of the setting.
fn run_command(command: &[String]) -> Result<PathBuf, PnpmExecCommandError> {
    let joined = display_command(command);
    let (program, args) = command.split_first().expect("command was validated as non-empty");
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| PnpmExecCommandError::Fail {
            command: joined.clone(),
            status_suffix: String::new(),
            hint: Some(error.to_string()),
        })?;

    // Drain stdout on its own thread while waiting, so a resolver that
    // writes more than the OS pipe buffer blocks neither itself nor the
    // wait below (it would otherwise sit wedged until the timeout).
    let reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut stdout = Vec::new();
            let _ = pipe.read_to_end(&mut stdout);
            stdout
        })
    });

    let status = wait_with_timeout(&mut child, COMMAND_TIMEOUT).map_err(|_elapsed| {
        PnpmExecCommandError::Fail {
            command: joined.clone(),
            status_suffix: String::new(),
            hint: Some(format!("timed out after {} seconds", COMMAND_TIMEOUT.as_secs())),
        }
    })?;

    // The child has exited (or been killed), so its end of the pipe is
    // closed and the reader finishes promptly.
    let stdout = reader.and_then(|handle| handle.join().ok()).unwrap_or_default();

    if !status.success() {
        return Err(PnpmExecCommandError::Fail {
            command: joined,
            status_suffix: status
                .code()
                .map(|code| format!(" with exit code {code}"))
                .unwrap_or_default(),
            hint: None,
        });
    }

    let bin_path = String::from_utf8_lossy(&stdout).trim().to_string();
    if bin_path.is_empty() {
        return Err(PnpmExecCommandError::NoOutput { command: joined });
    }
    let bin_path = PathBuf::from(bin_path);
    if !bin_path.is_absolute() {
        return Err(PnpmExecCommandError::RelativePath {
            command: joined,
            path: bin_path.display().to_string(),
        });
    }
    if !bin_path.is_file() {
        return Err(PnpmExecCommandError::BadPath {
            command: joined,
            path: bin_path.display().to_string(),
        });
    }
    Ok(bin_path)
}

/// Wait for `child`, killing it and returning `Err` when `timeout`
/// elapses first.
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, Duration> {
    let started = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(timeout);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn is_current_binary(bin_path: &Path) -> bool {
    let Ok(current) = std::env::current_exe() else {
        return false;
    };
    match (dunce::canonicalize(bin_path), dunce::canonicalize(&current)) {
        (Ok(resolved), Ok(current)) => resolved == current,
        _ => false,
    }
}

/// Re-exec the current invocation through the pnpm binary at
/// `bin_path`, then exit with the child's status. The child inherits
/// the [`PNPM_EXEC_PATH_ENV`] sentinel (so it skips re-resolution) and
/// `bin_path`'s directory prepended to `PATH` (so nested `pnpm`
/// invocations from lifecycle scripts resolve to the same binary).
fn re_exec(bin_path: &Path, command: &[String]) -> Result<(), PnpmExecCommandError> {
    let bin_dir = bin_path.parent().unwrap_or_else(|| Path::new("/"));
    let target =
        format!(r#"the binary resolved by pnpmExecCommand ("{}")"#, display_command(command));

    let depth: u32 =
        std::env::var(RE_EXEC_DEPTH_ENV).ok().and_then(|d| d.parse().ok()).unwrap_or(0);
    if depth >= MAX_RE_EXEC_DEPTH {
        return Err(PnpmExecCommandError::VersionSwitchFail {
            target,
            bin_dir: bin_dir.display().to_string(),
            hint: Some(format!(
                "re-exec depth exceeded {MAX_RE_EXEC_DEPTH}; the binary keeps redirecting to a different one"
            )),
        });
    }

    // Spawn the exact resolved file path rather than relying on PATH
    // resolution, so a broken bin dir cannot silently fall through to a
    // different pnpm (see pnpm/pnpm#8679 for the fork-bomb this
    // prevents on the TypeScript side).
    let status = Command::new(bin_path)
        .args(std::env::args_os().skip(1))
        .env_remove("PATH")
        .env_remove("Path")
        .env("PATH", prepend_to_path(bin_dir, std::env::var_os("PATH")))
        .env(RE_EXEC_DEPTH_ENV, (depth + 1).to_string())
        .env(PNPM_EXEC_PATH_ENV, bin_path)
        .status()
        .map_err(|error| PnpmExecCommandError::VersionSwitchFail {
            target,
            bin_dir: bin_dir.display().to_string(),
            hint: Some(error.to_string()),
        })?;

    #[expect(
        clippy::exit,
        reason = "the re-exec'd child owns the invocation; this process only relays its exit code"
    )]
    std::process::exit(status.code().unwrap_or(1));
}

/// Prepend `dir` to `current` (the inherited `PATH`), unless it
/// already leads it. `current` is a parameter rather than read here so
/// the function is testable without process-env access.
fn prepend_to_path(dir: &Path, current: Option<OsString>) -> OsString {
    let delimiter = if cfg!(windows) { ";" } else { ":" };
    let current = current.filter(|value| !value.is_empty());
    if let Some(current) = &current {
        let leading = {
            let mut prefix = OsString::from(dir);
            prefix.push(delimiter);
            prefix
        };
        if current == dir.as_os_str()
            || current.as_encoded_bytes().starts_with(leading.as_encoded_bytes())
        {
            return current.clone();
        }
    }
    let mut out = OsString::from(dir);
    if let Some(current) = current {
        out.push(delimiter);
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests;
