//! The dispatcher behind context-aware global shims.
//!
//! Every bin pnpm links into the global bin dir invokes the adjacent
//! protocol-versioned executable with
//! `--shim <name> <shim> <global-target> -- <args...>` (see
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
//! candidate must resolve through its aliases to a package whose manifest
//! name matches the global provider. Second, the exact candidate must be
//! approved on the terminal (`Do you trust this project?`). Answers are
//! persisted in a machine-local registry and bound to the provider manifest,
//! canonical target, shim contents, and file identity, so a changed checkout
//! or replacement bin cannot inherit an earlier approval.
//! Without a terminal the dispatcher falls back to the global target.

use pacquet_cmd_shim::CONTEXT_AWARE_DISPATCHER_NAME;
use pacquet_config::{
    Host, WorkspaceSettings, default_config_dir, default_pnpm_home_dir, default_state_dir,
};
use pacquet_crypto_hash::{create_hex_hash, create_hex_hash_bytes, create_hex_hash_from_file};
use pacquet_fs::lexical_normalize;
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

/// The trust registry, one JSON record per line, last matching record wins:
/// `{"projectDir": <abs path>, "candidateId": <sha256>, "allow": bool,
/// "decidedAt": <ms since epoch>}`. Corrupt or interleaved lines are ignored,
/// so a concurrent append can only make the dispatcher ask again.
const TRUST_FILE_NAME: &str = "global-bin-trust.jsonl";
const MAX_HASHED_BIN_SIZE: u64 = 1024 * 1024;

pub(crate) fn install_dispatcher(global_bin_dir: &Path) -> std::io::Result<()> {
    let source = std::env::current_exe()?;
    let file_name = if cfg!(windows) {
        format!("{CONTEXT_AWARE_DISPATCHER_NAME}.exe")
    } else {
        CONTEXT_AWARE_DISPATCHER_NAME.to_string()
    };
    install_dispatcher_from(&source, &global_bin_dir.join(file_name))
}

fn install_dispatcher_from(source: &Path, destination: &Path) -> std::io::Result<()> {
    crate::executable_link::replace_executable(source, destination)
}

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
    let Some((name, shim_path, global_target, args)) = parse_shim_argv(rest) else {
        eprintln!(
            "pnpm: malformed --shim invocation. Usage: pnpm --shim <name> <shim> <target> -- [args...]",
        );
        return 1;
    };
    if bypass_requested() || !global_shims_enabled() {
        return run_global_fallback(shim_path, global_target, args);
    }
    let candidate = std::env::current_dir()
        .ok()
        .and_then(|cwd| find_candidate(&cwd, name))
        .and_then(|candidate| validate_candidate(candidate, global_target, name));
    match candidate {
        Some(candidate) if is_trusted(&candidate, name) => match candidate {
            Candidate::LocalBin { bin, .. } => exec_program(&bin, args),
            Candidate::RuntimePin { version_spec, .. } => {
                run_runtime_via_dlx(name, &version_spec, args)
            }
        },
        _ => run_global_fallback(shim_path, global_target, args),
    }
}

/// Re-enter the generated shim with dispatch disabled so its original
/// direct-exec body performs the fallback. This preserves the shell's exact
/// shebang-argument parsing and interpreter lookup instead of reconstructing
/// either from a whitespace-split string. A concurrently removed shim falls
/// back to executing the embedded target directly.
fn run_global_fallback(shim_path: &Path, target: &Path, args: &[OsString]) -> i32 {
    if shim_path.is_file() {
        return exec_program_with_bypass(shim_path, args);
    }
    exec_program(target, args)
}

/// Split the machine-generated tail of a `--shim` invocation:
/// `<name> <shim> <target> -- [args...]`.
fn parse_shim_argv(rest: &[OsString]) -> Option<(&str, &Path, &Path, &[OsString])> {
    let [name, shim, target, separator, args @ ..] = rest else {
        return None;
    };
    if separator.to_str() != Some("--") {
        return None;
    }
    Some((name.to_str()?, Path::new(shim), Path::new(target), args))
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
    let global_config = default_config_dir::<Host>()
        .and_then(|config_dir| WorkspaceSettings::load_global(&config_dir).ok().flatten())
        .and_then(|settings| settings.global_shims);
    default_pnpm_home_dir::<Host>()
        .and_then(|home| WorkspaceSettings::find_and_load(&home).ok().flatten())
        .and_then(|(_, settings)| settings.global_shims)
        .or(global_config)
        .unwrap_or(true)
}

/// The context switch only ever substitutes a different version of the
/// same package the user installed globally: the local candidate must be
/// provided by the same-named package as the embedded global target.
/// A project shipping a same-named bin from a *different* package (a
/// lookalike `tsc` from `evil-pkg`) fails the match and the global
/// version runs.
fn validate_candidate(candidate: Candidate, global_target: &Path, name: &str) -> Option<Candidate> {
    let global_provider = provider_of_target(global_target)?;
    match candidate {
        Candidate::LocalBin { project_dir, bin, .. } => {
            let local = local_bin_identity(&bin, name)?;
            (local.provider.name == global_provider.name).then_some(Candidate::LocalBin {
                project_dir,
                bin,
                identity: local.fingerprint,
            })
        }
        Candidate::RuntimePin { project_dir, version_spec, manifest_hash, .. } => {
            (global_provider.name == name).then(|| Candidate::RuntimePin {
                project_dir,
                identity: create_hex_hash(&format!(
                    "runtime\0{name}\0{version_spec}\0{manifest_hash}",
                )),
                version_spec,
                manifest_hash,
            })
        }
    }
}

struct Provider {
    name: String,
    package_dir: PathBuf,
    manifest_hash: String,
}

struct LocalBinIdentity {
    provider: Provider,
    fingerprint: String,
}

/// Resolve a target through aliases/workspace links, then read the nearest
/// package manifest. Package identity comes from the manifest rather than the
/// attacker-controlled alias or shim path.
fn provider_of_target(target: &Path) -> Option<Provider> {
    let target = dunce::canonicalize(target).ok()?;
    let package_dir = package_dir_of_target(&target)?;
    let manifest = std::fs::read(package_dir.join("package.json")).ok()?;
    let parsed: Value = serde_json::from_slice(&manifest).ok()?;
    let name = parsed.get("name").and_then(Value::as_str)?.to_string();
    Some(Provider { name, package_dir, manifest_hash: create_hex_hash_bytes(&manifest) })
}

fn package_dir_of_target(target: &Path) -> Option<PathBuf> {
    for dir in target.parent()?.ancestors() {
        if dir.join("package.json").is_file() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

fn local_bin_identity(bin: &Path, name: &str) -> Option<LocalBinIdentity> {
    let metadata = std::fs::symlink_metadata(bin).ok()?;
    let (target, bin_hash) = if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(bin).ok()?;
        let hash = create_hex_hash(&format!("symlink\0{}", target.display()));
        (target, hash)
    } else {
        let script = bin.parent()?.join(name);
        let content = std::fs::read_to_string(&script).ok()?;
        let target = read_shim_target_from_content(&content)?;
        (target, create_hex_hash(&content))
    };
    let resolved = if target.is_absolute() { target } else { bin.parent()?.join(target) };
    let provider = provider_of_target(&resolved)?;
    let target = dunce::canonicalize(resolved).ok()?;
    let target_stat = file_identity(&target)?;
    let lockfile_hash = project_lockfile_hash(bin);
    let fingerprint = create_hex_hash(&format!(
        "bin\0{name}\0{}\0{}\0{}\0{}\0{}\0{bin_hash}\0{lockfile_hash}",
        provider.name,
        provider.package_dir.display(),
        provider.manifest_hash,
        target.display(),
        target_stat,
    ));
    Some(LocalBinIdentity { provider, fingerprint })
}

fn project_lockfile_hash(path: &Path) -> String {
    path.ancestors()
        .find_map(|dir| create_hex_hash_from_file(&dir.join("pnpm-lock.yaml")).ok())
        .unwrap_or_else(|| "none".to_string())
}

fn file_identity(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    #[cfg(unix)]
    let platform_identity = {
        use std::os::unix::fs::MetadataExt as _;
        format!("{}:{}", metadata.dev(), metadata.ino())
    };
    #[cfg(windows)]
    let platform_identity = windows_file_identity(path)?;
    #[cfg(not(any(unix, windows)))]
    let platform_identity = "0";
    let content_hash = small_file_hash(path, metadata.len()).unwrap_or_else(|| "large".to_string());
    Some(format!("{}:{modified_ns}:{platform_identity}:{content_hash}", metadata.len()))
}

fn small_file_hash(path: &Path, expected_len: u64) -> Option<String> {
    use std::io::Read as _;

    if expected_len > MAX_HASHED_BIN_SIZE {
        return None;
    }
    let mut bytes = Vec::with_capacity(expected_len as usize);
    std::fs::File::open(path).ok()?.take(MAX_HASHED_BIN_SIZE + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_HASHED_BIN_SIZE).then(|| create_hex_hash_bytes(&bytes))
}

#[cfg(windows)]
fn windows_file_identity(path: &Path) -> Option<String> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = std::fs::File::open(path).ok()?;
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: `file` owns a valid handle for this call and `info` points to
    // writable storage of the exact structure the API initializes.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) } == 0 {
        return None;
    }
    // SAFETY: a successful `GetFileInformationByHandle` initializes `info`.
    let info = unsafe { info.assume_init() };
    Some(format!("{}:{}:{}", info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
}

fn read_shim_target_from_content(content: &str) -> Option<PathBuf> {
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
    LocalBin { project_dir: PathBuf, bin: PathBuf, identity: String },
    /// The project pins the runtime `<name>` in `devEngines.runtime` /
    /// `engines.runtime` but has not materialized it; the pinned version
    /// is fetched into the store on demand.
    RuntimePin {
        project_dir: PathBuf,
        version_spec: String,
        manifest_hash: String,
        identity: String,
    },
}

impl Candidate {
    fn project_dir(&self) -> &Path {
        match self {
            Candidate::LocalBin { project_dir, .. } | Candidate::RuntimePin { project_dir, .. } => {
                project_dir
            }
        }
    }

    fn identity(&self) -> &str {
        match self {
            Candidate::LocalBin { identity, .. } | Candidate::RuntimePin { identity, .. } => {
                identity
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
            return Some(Candidate::LocalBin {
                project_dir: dir.to_path_buf(),
                bin,
                identity: String::new(),
            });
        }
        if runtime && let Some((version_spec, manifest_hash)) = manifest_runtime_pin(dir, name) {
            return Some(Candidate::RuntimePin {
                project_dir: dir.to_path_buf(),
                version_spec,
                manifest_hash,
                identity: String::new(),
            });
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
fn manifest_runtime_pin(dir: &Path, name: &str) -> Option<(String, String)> {
    let bytes = std::fs::read(dir.join("package.json")).ok()?;
    let manifest_hash = create_hex_hash_bytes(&bytes);
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
                    return Some((version.to_string(), manifest_hash));
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
    let candidate_id = candidate.identity();
    let project_key = lexical_normalize(project_dir).display().to_string();
    let trust_file = default_state_dir::<Host>().map(|dir| dir.join(TRUST_FILE_NAME));
    if let Some(trust_file) = &trust_file
        && let Some(allow) = read_trust_decision(trust_file, &project_key, candidate_id)
    {
        return allow;
    }
    let Some(allow) = prompt_for_trust(&project_key, name) else {
        return false;
    };
    if let Some(trust_file) = &trust_file {
        // Best-effort: an unwritable state dir means re-prompting next
        // time, which is strictly better than failing the command.
        let _ = append_trust_decision(trust_file, &project_key, candidate_id, allow);
    }
    allow
}

/// The recorded decision for `project_key`, last record wins.
fn read_trust_decision(trust_file: &Path, project_key: &str, candidate_id: &str) -> Option<bool> {
    let content = std::fs::read_to_string(trust_file).ok()?;
    let mut decision = None;
    for line in content.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if record.get("projectDir").and_then(Value::as_str) == Some(project_key)
            && record.get("candidateId").and_then(Value::as_str) == Some(candidate_id)
        {
            decision = record.get("allow").and_then(Value::as_bool);
        }
    }
    decision
}

fn append_trust_decision(
    trust_file: &Path,
    project_key: &str,
    candidate_id: &str,
    allow: bool,
) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = trust_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let decided_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    let record = json!({
        "projectDir": project_key,
        "candidateId": candidate_id,
        "allow": allow,
        "decidedAt": decided_at,
    });
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

#[cfg(unix)]
fn exec_program_with_bypass(program: &Path, args: &[OsString]) -> i32 {
    use std::os::unix::process::CommandExt as _;
    let error = Command::new(program).args(args).env(BYPASS_ENV, "1").exec();
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

#[cfg(windows)]
fn exec_program_with_bypass(program: &Path, args: &[OsString]) -> i32 {
    let extension = program.extension().and_then(|ext| ext.to_str()).unwrap_or_default();
    let mut command =
        if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat") {
            let mut command = Command::new("cmd");
            command.arg("/c").arg(program);
            command
        } else if extension.eq_ignore_ascii_case("ps1") {
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(program);
            command
        } else {
            Command::new(program)
        };
    match command.args(args).env(BYPASS_ENV, "1").status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("pnpm: failed to run {}: {error}", program.display());
            if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
        }
    }
}

#[cfg(test)]
mod tests;
