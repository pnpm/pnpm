//! The dispatcher behind context-aware global shims.
//!
//! Selected bins pnpm links into the global bin dir invoke the adjacent
//! protocol-versioned executable with
//! `--shim <name> <shim> <global-target> -- <args...>` (see
//! `pacquet_cmd_shim::ShimStyle`). In the default `auto` mode only an
//! authenticated Node runtime is context-aware: the dispatcher reads the
//! project's `devEngines.runtime` / `engines.runtime` pin, materializes the
//! publisher-signature-verified release in pnpm's global virtual store, and
//! executes it directly. It never resolves a runtime through the project's
//! `node_modules/.bin` directory.
//!
//! `globalShims: all` also permits ordinary package bins to switch. That is
//! a trust decision, gated twice. First, the
//! candidate must resolve through its aliases to a package whose manifest
//! name matches the global provider. Second, the exact candidate must be
//! approved on the terminal (`Do you trust this project?`). Answers are
//! persisted in a machine-local registry and bound to the provider manifest,
//! canonical target, shim contents, and file identity, so a changed checkout
//! or replacement bin cannot inherit an earlier approval.
//! Without a terminal the dispatcher falls back to the global target.

use crate::{State, cli_args::add::add_package};
use miette::{Context, IntoDiagnostic};
use pacquet_cmd_shim::CONTEXT_AWARE_DISPATCHER_NAME;
use pacquet_config::{
    Config, GlobalShims, Host, WorkspaceSettings, default_config_dir, default_pnpm_home_dir,
    default_state_dir,
};
use pacquet_crypto_hash::{create_hex_hash, create_hex_hash_bytes, create_hex_hash_from_file};
use pacquet_engine_runtime_node_resolver::parse_node_specifier;
use pacquet_fs::{DirLock, lexical_normalize};
use pacquet_package_manifest::{DependencyGroup, is_runtime_alias};
use pacquet_registry::RangeSpecStyle;
use pacquet_reporter::SilentReporter;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
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
const RUNTIME_ENVS_DIR_NAME: &str = "global-shim-runtimes";
#[cfg(windows)]
const WINDOWS_NODE_TARGET_FILE_NAME: &str = ".pnpm-shim-v1-node-target";
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

#[cfg(windows)]
pub(crate) fn install_windows_node_dispatcher(
    global_bin_dir: &Path,
    global_target: &Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    let dispatcher = global_bin_dir.join(format!("{CONTEXT_AWARE_DISPATCHER_NAME}.exe"));
    let node_exe = global_bin_dir.join("node.exe");
    let target_file = global_bin_dir.join(WINDOWS_NODE_TARGET_FILE_NAME);
    let staged_target =
        global_bin_dir.join(format!(".{WINDOWS_NODE_TARGET_FILE_NAME}.{}.tmp", std::process::id()));
    let encoded =
        global_target.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    fs::write(&staged_target, encoded)?;
    let publish_target = crate::executable_link::replace_executable(&staged_target, &target_file);
    let _ = fs::remove_file(&staged_target);
    publish_target?;
    crate::executable_link::replace_executable(&dispatcher, &node_exe)
}

/// Intercept a `pnpm --shim ...` invocation. `None` means argv is not a
/// shim dispatch and the regular CLI should proceed; `Some(code)` means
/// the dispatch ran (or failed) and the process must exit with `code`.
/// On Unix a successful dispatch never returns at all — the target is
/// `exec`ed in place.
pub(crate) fn try_dispatch(argv: &[OsString]) -> Option<i32> {
    if argv.get(1).and_then(|arg| arg.to_str()) == Some("--shim") {
        return Some(dispatch(&argv[2..]));
    }
    #[cfg(windows)]
    if let Some(result) = try_windows_node_dispatch(argv) {
        return Some(result);
    }
    None
}

fn dispatch(rest: &[OsString]) -> i32 {
    let Some((name, shim_path, global_target, args)) = parse_shim_argv(rest) else {
        eprintln!(
            "pnpm: malformed --shim invocation. Usage: pnpm --shim <name> <shim> <target> -- [args...]",
        );
        return 1;
    };
    let mode = global_shims_mode();
    dispatch_target(name, Some(shim_path), global_target, args, mode)
}

fn dispatch_target(
    name: &str,
    shim_path: Option<&Path>,
    global_target: &Path,
    args: &[OsString],
    mode: GlobalShims,
) -> i32 {
    if bypass_requested() || mode == GlobalShims::Off {
        return run_global_fallback(shim_path, global_target, args);
    }
    let candidate = std::env::current_dir()
        .ok()
        .and_then(|cwd| find_candidate(&cwd, name, mode))
        .and_then(|candidate| validate_candidate(candidate, global_target, name));
    match candidate {
        Some(Candidate::RuntimePin { project_dir, version_spec, .. })
            if is_automatic_runtime(name, &version_spec) =>
        {
            run_runtime_from_store(name, &version_spec, &project_dir, args)
        }
        Some(candidate) if mode == GlobalShims::All && is_trusted(&candidate, name) => {
            match candidate {
                Candidate::LocalBin { bin, .. } => exec_program(&bin, args),
                Candidate::RuntimePin { project_dir, version_spec, .. } => {
                    run_runtime_from_store(name, &version_spec, &project_dir, args)
                }
            }
        }
        _ => run_global_fallback(shim_path, global_target, args),
    }
}

#[cfg(windows)]
fn try_windows_node_dispatch(argv: &[OsString]) -> Option<i32> {
    use std::os::windows::ffi::OsStringExt as _;

    let executable = std::env::current_exe().ok()?;
    if !executable
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("node.exe"))
    {
        return None;
    }
    let target_file = executable.parent()?.join(WINDOWS_NODE_TARGET_FILE_NAME);
    let global_target = match fs::read(&target_file).ok().and_then(|bytes| {
        let mut chunks = bytes.chunks_exact(2);
        let path = OsString::from_wide(
            &chunks
                .by_ref()
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>(),
        );
        chunks.remainder().is_empty().then(|| PathBuf::from(path))
    }) {
        Some(target) => target,
        None => {
            eprintln!("pnpm: cannot read the global Node.js target at {}", target_file.display());
            return Some(1);
        }
    };
    Some(dispatch_target("node", None, &global_target, &argv[1..], global_shims_mode()))
}

/// Re-enter the generated shim with dispatch disabled so its original
/// direct-exec body performs the fallback. This preserves the shell's exact
/// shebang-argument parsing and interpreter lookup instead of reconstructing
/// either from a whitespace-split string. A concurrently removed shim falls
/// back to executing the embedded target directly.
fn run_global_fallback(shim_path: Option<&Path>, target: &Path, args: &[OsString]) -> i32 {
    if let Some(shim_path) = shim_path.filter(|path| path.is_file()) {
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
fn global_shims_mode() -> GlobalShims {
    for env_name in ["PNPM_CONFIG_GLOBAL_SHIMS", "pnpm_config_global_shims"] {
        if let Ok(value) = std::env::var(env_name)
            && !value.is_empty()
        {
            if let Ok(mode) = serde_json::from_str::<GlobalShims>(&value) {
                return mode;
            }
            if let Ok(quoted) = serde_json::to_string(&value)
                && let Ok(mode) = serde_json::from_str::<GlobalShims>(&quoted)
            {
                return mode;
            }
        }
    }
    let global_config = default_config_dir::<Host>()
        .and_then(|config_dir| WorkspaceSettings::load_global(&config_dir).ok().flatten())
        .and_then(|settings| settings.global_shims);
    default_pnpm_home_dir::<Host>()
        .and_then(|home| WorkspaceSettings::find_and_load(&home).ok().flatten())
        .and_then(|(_, settings)| settings.global_shims)
        .or(global_config)
        .unwrap_or_default()
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

/// Walk up from `cwd` to the nearest directory providing `name`. Runtime
/// shims only consider manifest pins and never inspect `.bin`; ordinary
/// package bins are considered only in the explicit `all` mode. Directories
/// inside the pnpm home are skipped because global installs are not projects.
fn find_candidate(cwd: &Path, name: &str, mode: GlobalShims) -> Option<Candidate> {
    let pnpm_home = default_pnpm_home_dir::<Host>();
    let runtime = is_runtime_alias(name);
    for dir in cwd.ancestors() {
        if pnpm_home.as_deref().is_some_and(|home| dir.starts_with(home)) {
            continue;
        }
        if runtime && let Some((version_spec, manifest_hash)) = manifest_runtime_pin(dir, name) {
            return (mode == GlobalShims::All || is_automatic_runtime(name, &version_spec)).then(
                || Candidate::RuntimePin {
                    project_dir: dir.to_path_buf(),
                    version_spec,
                    manifest_hash,
                    identity: String::new(),
                },
            );
        }
        if !runtime
            && mode == GlobalShims::All
            && let Some(bin) = local_bin_path(dir, name)
        {
            return Some(Candidate::LocalBin {
                project_dir: dir.to_path_buf(),
                bin,
                identity: String::new(),
            });
        }
    }
    None
}

/// Stable Node releases are authenticated by the Node.js release-team keys
/// before the resolver admits their archive. Other Node channels and the
/// Deno/Bun resolvers currently rely on checksums alone, so they stay behind
/// the explicit `all` opt-in.
fn is_automatic_runtime(name: &str, version_spec: &str) -> bool {
    name == "node"
        && parse_node_specifier(version_spec)
            .is_ok_and(|specifier| specifier.release_channel == "release")
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

fn run_runtime_from_store(
    name: &str,
    version_spec: &str,
    project_dir: &Path,
    args: &[OsString],
) -> i32 {
    let result = crate::block_on_runtime(
        "pacquet-global-shim-runtime",
        materialize_runtime(name.to_string(), version_spec.to_string(), project_dir.to_path_buf()),
    );
    match result {
        Ok(bin) => exec_program(&bin, args),
        Err(error) => {
            eprintln!("pnpm: failed to prepare {name}@runtime:{version_spec}: {error:?}");
            1
        }
    }
}

/// Materialize a runtime into the configured global virtual store and return
/// its real executable. The small environment under pnpm's state directory
/// contains only the lockfile and symlinks required to address the GVS slot;
/// project `node_modules` is never consulted.
async fn materialize_runtime(
    name: String,
    version_spec: String,
    project_dir: PathBuf,
) -> miette::Result<PathBuf> {
    let config = Config::default()
        .current::<Host>(&project_dir)
        .map_err(miette::Report::new)
        .wrap_err("load configuration for the managed runtime")?;
    let state_dir = default_state_dir::<Host>()
        .ok_or_else(|| miette::miette!("the pnpm state directory could not be resolved"))?;
    let global_virtual_store_dir = config.store_dir.links();
    let key = create_hex_hash(&format!(
        "runtime\0{name}\0{version_spec}\0{}",
        global_virtual_store_dir.display(),
    ));
    let environments_dir = state_dir.join(RUNTIME_ENVS_DIR_NAME);
    fs::create_dir_all(&environments_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("create {}", environments_dir.display()))?;
    let environment_dir = environments_dir.join(&key);
    if let Some(bin) = managed_runtime_bin(&environment_dir, &name, &global_virtual_store_dir) {
        return Ok(bin);
    }

    const WAIT: Duration = Duration::from_mins(5);
    const ABANDONED_AFTER: Duration = Duration::from_mins(30);
    let lock_path = environments_dir.join(format!("{key}.lock"));
    let _lock = DirLock::acquire(lock_path.clone(), WAIT, ABANDONED_AFTER)
        .into_diagnostic()
        .wrap_err_with(|| format!("lock the managed runtime at {}", lock_path.display()))?;
    if let Some(bin) = managed_runtime_bin(&environment_dir, &name, &global_virtual_store_dir) {
        return Ok(bin);
    }

    remove_dir_if_not_symlink(&environment_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("reset {}", environment_dir.display()))?;
    fs::create_dir_all(&environment_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("create {}", environment_dir.display()))?;

    let mut install_config = config;
    install_config.modules_dir = environment_dir.join("node_modules");
    install_config.virtual_store_dir = environment_dir.join("node_modules").join(".pnpm");
    install_config.enable_global_virtual_store = true;
    install_config.global_virtual_store_dir = global_virtual_store_dir;
    install_config.workspace_dir = Some(environment_dir.clone());
    install_config.lockfile = true;
    install_config.frozen_lockfile = Some(false);
    install_config.prefer_frozen_lockfile = false;
    install_config.ignore_scripts = true;
    install_config.dangerously_allow_all_builds = false;
    install_config.strict_dep_builds = false;
    install_config.allow_builds.clear();
    install_config.overrides = None;
    install_config.package_extensions = None;
    install_config.catalogs = None;
    install_config.patched_dependencies = None;
    let install_config = Config::leak(install_config);
    let state = State::init(environment_dir.join("package.json"), install_config, false)
        .wrap_err("initialize the managed runtime environment")?;
    add_package::<SilentReporter, _>(
        state,
        &format!("{name}@runtime:{version_spec}"),
        RangeSpecStyle::Patch,
        None,
        false,
        install_config.supported_architectures.clone(),
        [DependencyGroup::Prod],
    )
    .await
    .wrap_err("install the managed runtime into the global virtual store")?;

    let global_virtual_store_dir_display = install_config.global_virtual_store_dir.display();
    managed_runtime_bin(&environment_dir, &name, &install_config.global_virtual_store_dir).ok_or_else(
        || {
            miette::miette!(
                "the installed {name} executable is not in the global virtual store at {global_virtual_store_dir_display}"
            )
        },
    )
}

fn managed_runtime_bin(
    environment_dir: &Path,
    name: &str,
    global_virtual_store_dir: &Path,
) -> Option<PathBuf> {
    let package_dir = dunce::canonicalize(environment_dir.join("node_modules").join(name)).ok()?;
    let store_dir = dunce::canonicalize(global_virtual_store_dir).ok()?;
    if !package_dir.starts_with(&store_dir) {
        return None;
    }
    let manifest: Value =
        serde_json::from_slice(&fs::read(package_dir.join("package.json")).ok()?).ok()?;
    if manifest.get("name").and_then(Value::as_str) != Some(name) {
        return None;
    }
    let bin_path = match manifest.get("bin")? {
        Value::String(path) => path.as_str(),
        Value::Object(bins) => bins.get(name)?.as_str()?,
        _ => return None,
    };
    let bin = dunce::canonicalize(package_dir.join(bin_path)).ok()?;
    (bin.starts_with(&package_dir) && bin.is_file()).then_some(bin)
}

fn remove_dir_if_not_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "managed runtime environment must not be a symbolic link",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    fs::remove_dir_all(path)
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
