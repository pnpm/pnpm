//! The dispatcher behind context-aware global shims.
//!
//! A context-aware shim is the pnpm executable published under the bin's
//! own name, with the global target recorded beside it (see
//! [`native_shim`]). Launched under that name, pnpm dispatches instead of
//! running the CLI: the `globalShims` record decides which providing
//! packages are eligible, and the managed runtimes are enabled by default.
//! For a runtime pin, the dispatcher reads the project's
//! `devEngines.runtime` / `engines.runtime`, materializes the release in
//! pnpm's global virtual store, and executes it directly — never through
//! the project's `node_modules/.bin`. A publisher-signature-verified
//! stable Node release runs without prompting.
//!
//! Everything else eligible — ordinary package bins, unsigned runtime
//! channels — is a trust decision, gated twice. First, the
//! candidate must resolve through its aliases to a package whose manifest
//! name matches the global provider. Second, the exact candidate must be
//! approved on the terminal (`Do you trust this project?`). Answers are
//! persisted in a machine-local registry and bound to the provider manifest,
//! canonical target, shim contents, and file identity, so a changed checkout
//! or replacement bin cannot inherit an earlier approval.
//! Without a terminal the dispatcher falls back to the global target.

use crate::{
    cli_args::package_manager::wanted_package_manager,
    engine_pm::{
        channel::{Channel, PackageManager},
        provision::provision,
    },
};
use derive_more::Display;
use pnpm_cmd_shim::{Host as CmdShimHost, ScriptRuntime, search_script_runtime};
use pnpm_config::{
    Config, GlobalShims, GlobalShimsSetting, Host, LoadWorkspaceYamlError, ShimPolicy,
    WorkspaceSettings, default_config_dir, default_pnpm_home_dir, default_state_dir,
    resolve_configured_state_dir,
};
use pnpm_crypto_hash::{create_hex_hash, create_hex_hash_bytes};
use pnpm_engine_runtime_node_resolver::parse_node_specifier;
use pnpm_package_manifest::is_runtime_alias;
use pnpm_reporter::SilentReporter;
use serde_json::Value;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

mod identity;
pub(crate) mod native_shim;
pub(crate) mod runtime_env;
mod trust;

pub(crate) use native_shim::{
    ShimTarget, install_native_shim, is_legacy_context_aware_shim, migrate_legacy_shims,
    native_shim_is_installed, native_shim_paths, native_shim_target, native_shims,
    refresh_native_shims, remove_native_shim,
};
pub(crate) use runtime_env::materialize_runtime;

use identity::{local_bin_identity, provider_of_target};
use native_shim::{dispatch_legacy_shim, try_native_dispatch};
use runtime_env::{PACKAGE_MANAGER_ENVS_DIR_NAME, trusted_runtime_config};
use trust::is_trusted;

/// Environment variable that short-circuits the dispatcher to the global
/// target: a user-facing kill switch that the target's own children, a
/// sibling `node` shim included, inherit.
const BYPASS_ENV: &str = "PNPM_SHIM_BYPASS";

/// Intercept a launch under a shim name, or a legacy shim's `--shim`
/// invocation of the dispatcher it replaced. `None` means this is pnpm
/// itself and the regular CLI should proceed; `Some(code)` means the
/// dispatch ran (or failed) and the process must exit with `code`. On Unix
/// a successful dispatch never returns at all — the target is `exec`ed in
/// place.
pub(crate) fn try_dispatch(argv: &[OsString]) -> Option<i32> {
    if argv.get(1).and_then(|arg| arg.to_str()) == Some("--shim") {
        return Some(dispatch_legacy_shim(&argv[2..]));
    }
    try_native_dispatch(argv)
}

/// The shim being dispatched: its bin name, the directory it lives in
/// (where a sibling interpreter such as `node` is looked up first), and
/// the global target it falls back to.
struct ShimInvocation<'a> {
    name: &'a str,
    bin_dir: &'a Path,
    target: &'a ShimTarget,
}

fn dispatch_target(
    shim: &ShimInvocation<'_>,
    args: &[OsString],
    shims: &GlobalShims,
    state_dir: &Path,
) -> i32 {
    let ShimInvocation { name, target, .. } = *shim;
    if bypass_requested() || shims.dispatches_nothing() {
        return run_global_target(shim, args);
    }
    // Eligibility is keyed by the package the shim stands for, so an entry
    // for `typescript` covers its `tsc` bin. A shim with a global install
    // behind it takes that package from the target's manifest, which also
    // anchors the candidate match; a target-less shim declares it.
    let Some(package) = (match target {
        ShimTarget::Virtual(package) => Some(package.clone()),
        ShimTarget::Installed(path) => provider_of_target(path).map(|provider| provider.name),
    }) else {
        return run_global_target(shim, args);
    };
    let policy = shims.policy(&package);
    if policy == ShimPolicy::Off {
        return run_global_target(shim, args);
    }
    let candidate = std::env::current_dir()
        .ok()
        .and_then(|cwd| find_candidate(&cwd, name, &package))
        .and_then(|candidate| validate_candidate(candidate, &package, name));
    match candidate {
        Some(Candidate::RuntimePin { version_spec, .. })
            if runtime_runs_promptless(policy, name, &version_spec) =>
        {
            run_runtime_from_store(state_dir, name, &version_spec, args)
        }
        Some(Candidate::PackageManagerPin { pm, version_spec, .. })
            if package_manager_runs_promptless(policy, pm, &version_spec) =>
        {
            run_package_manager_from_pin(state_dir, pm, &version_spec, name, args)
        }
        Some(candidate)
            if policy == ShimPolicy::Always || is_trusted(&candidate, name, state_dir) =>
        {
            match candidate {
                Candidate::LocalBin { bin, identity, .. } => {
                    // The trust prompt leaves a human-scale window between
                    // fingerprinting and execution; revalidate so the
                    // approved bytes are the ones that run. The remaining
                    // race is process-scale and needs an attacker already
                    // executing code as the user — outside this gate's
                    // threat model, since a hostile repository is static.
                    if !local_bin_unchanged(&bin, name, &identity) {
                        return run_global_target(shim, args);
                    }
                    exec_program(&bin, args)
                }
                Candidate::RuntimePin { version_spec, .. } => {
                    run_runtime_from_store(state_dir, name, &version_spec, args)
                }
                Candidate::PackageManagerPin { pm, version_spec, .. } => {
                    run_package_manager_from_pin(state_dir, pm, &version_spec, name, args)
                }
            }
        }
        _ => run_global_target(shim, args),
    }
}

/// Whether the bin still carries the fingerprint the trust decision was
/// made against.
fn local_bin_unchanged(bin: &Path, name: &str, approved_identity: &str) -> bool {
    local_bin_identity(bin, name).is_some_and(|current| current.fingerprint == approved_identity)
}

/// Whether a runtime pin may run without the trust gate.
fn runtime_runs_promptless(policy: ShimPolicy, name: &str, version_spec: &str) -> bool {
    match policy {
        ShimPolicy::Always => true,
        ShimPolicy::Auto => is_automatic_runtime(name, version_spec),
        ShimPolicy::Off | ShimPolicy::Prompt => false,
    }
}

/// Run the global target the way a direct cmd-shim would: a script runs
/// under the interpreter its shebang names, taken from the bin dir when a
/// sibling of that name is installed there (so a global tool runs on the
/// global `node` shim) and from `PATH` otherwise; anything else executes
/// as it is. A target-less shim has nothing to run and says so.
fn run_global_target(shim: &ShimInvocation<'_>, args: &[OsString]) -> i32 {
    let target = match shim.target {
        ShimTarget::Installed(target) => target,
        ShimTarget::Virtual(package) => {
            eprintln!(
                r#"ERR_PNPM_SHIM_NO_TARGET  Nothing provides {} in this project. Add {package} to it, or pin it with "packageManager" in package.json."#,
                shim.name,
            );
            return 1;
        }
    };
    match search_script_runtime::<CmdShimHost>(target) {
        // `.cmd` and `.bat` targets go to `Command::new` directly (see
        // `exec_program_with_path`) rather than through an explicit `cmd`.
        Ok(Some(ScriptRuntime { prog: Some(prog), args: shebang_args })) if prog != "cmd" => {
            let mut argv = split_shebang_args(&shebang_args);
            argv.push(target.into());
            argv.extend_from_slice(args);
            exec_program(&interpreter_path(shim.bin_dir, &prog), &argv)
        }
        Ok(_) => exec_program(target, args),
        Err(error) => {
            eprintln!("pnpm: failed to read {}: {error}", target.display());
            126
        }
    }
}

/// Where the shebang's interpreter comes from: the bin dir's own entry
/// when there is one, else the bare name for a `PATH` lookup. An absolute
/// interpreter path joins to itself.
fn interpreter_path(bin_dir: &Path, prog: &str) -> PathBuf {
    let sibling = bin_dir.join(prog);
    if sibling.is_file() {
        return sibling;
    }
    if cfg!(windows) {
        let sibling = bin_dir.join(format!("{prog}.exe"));
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from(prog)
}

/// The interpreter arguments a shebang carries, split the way the shell
/// running a cmd-shim would split them. A line the shell could not parse
/// (an unbalanced quote) falls back to whitespace splitting.
fn split_shebang_args(shebang_args: &str) -> Vec<OsString> {
    let words = shell_words::split(shebang_args)
        .unwrap_or_else(|_| shebang_args.split_whitespace().map(str::to_string).collect());
    words.into_iter().map(OsString::from).collect()
}

fn bypass_requested() -> bool {
    std::env::var(BYPASS_ENV)
        .is_ok_and(|value| !value.is_empty() && value != "0" && value != "false")
}

/// The `globalShims` setting at dispatch time, so config edits take
/// effect immediately instead of waiting for the next global install to
/// relink the shims. Layers merge key-wise over the built-in defaults in
/// the order global `config.yaml`, the pnpm home's own
/// `pnpm-workspace.yaml`, then the env override — never a project file
/// and never a discovered ancestor of the pnpm home. (The env override
/// is only as trustworthy as the environment itself; tools like direnv
/// can scope it per directory.) The state directory comes from its default,
/// the global config, and the environment; the pnpm-home manifest cannot
/// redirect machine state.
struct TrustedShimSettings {
    shims: GlobalShims,
    state_dir: PathBuf,
}

fn trusted_shim_settings() -> TrustedShimSettings {
    match load_trusted_shim_settings() {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!(
                "pnpm: project-aware global shims are disabled because trusted configuration could not be loaded: {error}",
            );
            let mut shims = GlobalShims::default();
            shims.apply(&GlobalShimsSetting::Toggle(false));
            TrustedShimSettings {
                shims,
                state_dir: default_state_dir::<Host>().unwrap_or_default(),
            }
        }
    }
}

pub(crate) fn global_shims_setting() -> GlobalShims {
    trusted_shim_settings().shims
}

#[derive(Debug, Display)]
pub(crate) enum LoadGlobalShimsSettingError {
    #[display("{_0}")]
    Workspace(LoadWorkspaceYamlError),
    #[display("malformed {env_name} value {value:?}: {source}")]
    Environment { env_name: &'static str, value: String, source: serde_json::Error },
}

fn load_trusted_shim_settings() -> Result<TrustedShimSettings, LoadGlobalShimsSettingError> {
    let mut shims = GlobalShims::default();
    let default_state_dir = default_state_dir::<Host>().unwrap_or_default();
    let mut state_dir = default_state_dir.clone();
    if let Some(config_dir) = default_config_dir::<Host>() {
        let mut settings = WorkspaceSettings::load_global(&config_dir)
            .map_err(LoadGlobalShimsSettingError::Workspace)?;
        if let Some(settings) = settings.as_mut() {
            settings.substitute_env_trusted::<Host>();
            apply_state_dir_setting(
                &mut state_dir,
                settings.state_dir.as_deref(),
                &default_state_dir,
            );
            if let Some(layer) = settings.global_shims.as_ref() {
                shims.apply(layer);
            }
        }
    }
    apply_settings_above_global_config(&mut shims)?;
    let mut env_settings = WorkspaceSettings::from_pnpm_config_env::<Host>();
    env_settings.substitute_env_trusted::<Host>();
    apply_state_dir_setting(&mut state_dir, env_settings.state_dir.as_deref(), &default_state_dir);
    Ok(TrustedShimSettings { shims, state_dir })
}

fn apply_state_dir_setting(
    state_dir: &mut PathBuf,
    setting: Option<&str>,
    default_state_dir: &Path,
) {
    let Some(setting) = setting.filter(|setting| !setting.is_empty()) else { return };
    *state_dir = resolve_configured_state_dir(default_state_dir, setting);
}

/// Apply the `globalShims` layers that outrank the global `config.yaml`:
/// `$PNPM_HOME/pnpm-workspace.yaml`, then the environment.
///
/// `pnpm shim` uses this to answer whether the shim it is about to write
/// would ever dispatch, so the command and the dispatcher cannot disagree
/// about which settings are in force.
pub(crate) fn apply_settings_above_global_config(
    shims: &mut GlobalShims,
) -> Result<(), LoadGlobalShimsSettingError> {
    if let Some(home) = default_pnpm_home_dir::<Host>() {
        let settings =
            WorkspaceSettings::load_at(&home).map_err(LoadGlobalShimsSettingError::Workspace)?;
        if let Some(layer) = settings.and_then(|settings| settings.global_shims) {
            shims.apply(&layer);
        }
    }
    for env_name in ["PNPM_CONFIG_GLOBAL_SHIMS", "pnpm_config_global_shims"] {
        if let Ok(value) = std::env::var(env_name)
            && !value.is_empty()
        {
            let layer = serde_json::from_str::<GlobalShimsSetting>(&value).map_err(|source| {
                LoadGlobalShimsSettingError::Environment { env_name, value, source }
            })?;
            shims.apply(&layer);
            break;
        }
    }
    Ok(())
}

/// The context switch only ever substitutes a different version of the
/// same package the user installed globally: the local candidate must be
/// provided by the same-named package as the embedded global target.
/// A project shipping a same-named bin from a *different* package (a
/// lookalike `tsc` from `evil-pkg`) fails the match and the global
/// version runs.
fn validate_candidate(candidate: Candidate, package: &str, name: &str) -> Option<Candidate> {
    match candidate {
        Candidate::LocalBin { project_dir, bin, .. } => {
            let local = local_bin_identity(&bin, name)?;
            (local.provider.name == package).then_some(Candidate::LocalBin {
                project_dir,
                bin,
                identity: local.fingerprint,
            })
        }
        Candidate::RuntimePin { project_dir, version_spec, manifest_hash, .. } => (package == name)
            .then(|| Candidate::RuntimePin {
                project_dir,
                identity: create_hex_hash(&format!(
                    "runtime\0{name}\0{version_spec}\0{manifest_hash}",
                )),
                version_spec,
                manifest_hash,
            }),
        Candidate::PackageManagerPin { project_dir, pm, version_spec, manifest_hash, .. } => {
            (package == pm.name()).then(|| Candidate::PackageManagerPin {
                project_dir,
                identity: create_hex_hash(&format!(
                    "package-manager\0{}\0{version_spec}\0{manifest_hash}",
                    pm.name(),
                )),
                pm,
                version_spec,
                manifest_hash,
            })
        }
    }
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
    /// The project pins its package manager in `packageManager` /
    /// `devEngines.packageManager`. Like a runtime pin, the version is
    /// provisioned on demand rather than expected on the host.
    PackageManagerPin {
        project_dir: PathBuf,
        pm: PackageManager,
        version_spec: String,
        manifest_hash: String,
        identity: String,
    },
}

impl Candidate {
    fn project_dir(&self) -> &Path {
        match self {
            Candidate::LocalBin { project_dir, .. }
            | Candidate::RuntimePin { project_dir, .. }
            | Candidate::PackageManagerPin { project_dir, .. } => project_dir,
        }
    }

    fn identity(&self) -> &str {
        match self {
            Candidate::LocalBin { identity, .. }
            | Candidate::RuntimePin { identity, .. }
            | Candidate::PackageManagerPin { identity, .. } => identity,
        }
    }
}

/// Walk up from `cwd` to the nearest directory providing `name`, the bin
/// that was invoked, on behalf of `package`.
///
/// Runtime shims only consider manifest pins and never inspect `.bin`;
/// a package manager's pin outranks an installed copy of itself, because
/// the pin is the project's own statement of what installs it; ordinary
/// package bins resolve through `node_modules/.bin`. Directories inside
/// the pnpm home are skipped because global installs are not projects.
fn find_candidate(cwd: &Path, name: &str, package: &str) -> Option<Candidate> {
    let pnpm_home = default_pnpm_home_dir::<Host>();
    let runtime = is_runtime_alias(name);
    let package_manager = PackageManager::parse(package);
    for dir in cwd.ancestors() {
        if pnpm_home.as_deref().is_some_and(|home| dir.starts_with(home)) {
            continue;
        }
        if runtime && let Some((version_spec, manifest_hash)) = manifest_runtime_pin(dir, name) {
            return Some(Candidate::RuntimePin {
                project_dir: dir.to_path_buf(),
                version_spec,
                manifest_hash,
                identity: String::new(),
            });
        }
        if let Some(pm) = package_manager
            && let Some((version_spec, manifest_hash)) = manifest_package_manager_pin(dir, pm)
        {
            return Some(Candidate::PackageManagerPin {
                project_dir: dir.to_path_buf(),
                pm,
                version_spec,
                manifest_hash,
                identity: String::new(),
            });
        }
        if !runtime && let Some(bin) = local_bin_path(dir, name) {
            return Some(Candidate::LocalBin {
                project_dir: dir.to_path_buf(),
                bin,
                identity: String::new(),
            });
        }
    }
    None
}

/// The version a project's `package.json` pins for the package manager
/// `pm`, with the manifest's hash so an approval is bound to the file it
/// was given for. A pin naming a different package manager is not this
/// shim's business, and a pin without a version cannot be provisioned.
fn manifest_package_manager_pin(dir: &Path, pm: PackageManager) -> Option<(String, String)> {
    let bytes = std::fs::read(dir.join("package.json")).ok()?;
    let manifest_hash = create_hex_hash_bytes(&bytes);
    let manifest: Value = serde_json::from_slice(&bytes).ok()?;
    let wanted = wanted_package_manager(&manifest)?;
    (wanted.name == pm.name()).then_some(())?;
    Some((wanted.version?, manifest_hash))
}

/// Whether a package-manager pin may run without the trust gate.
///
/// A package manager published to npm is verified against npm's own
/// signature for its exact `name@version` before it executes — the same
/// standard that lets pnpm switch itself to a project's pin without
/// asking. Bun and Yarn 6 ship as release archives pinned by a publisher
/// checksum, which authenticates the bytes but not the publisher, so they
/// stay behind the gate like the checksum-only runtime channels.
fn package_manager_runs_promptless(
    policy: ShimPolicy,
    pm: PackageManager,
    version_spec: &str,
) -> bool {
    match policy {
        ShimPolicy::Always => true,
        ShimPolicy::Auto => matches!(pm.channel(version_spec), Channel::Registry { .. }),
        ShimPolicy::Off | ShimPolicy::Prompt => false,
    }
}

/// Stable Node releases are authenticated by the Node.js release-team keys
/// before the resolver admits their archive. Other Node channels and the
/// Deno/Bun resolvers currently rely on checksums alone, so they stay behind
/// the explicit `all` opt-in.
fn is_automatic_runtime(name: &str, version_spec: &str) -> bool {
    // On musl hosts the matching assets come from unofficial-builds
    // without signature verification, so a stable pin is not
    // publisher-authenticated there and stays behind the trust gate.
    name == "node"
        && parse_node_specifier(version_spec)
            .is_ok_and(|specifier| specifier.release_channel == "release")
        && pnpm_detect_libc::detect() != Some(pnpm_detect_libc::Implementation::Musl)
}

/// The runnable `node_modules/.bin` entry for `name` under `dir`, if any.
/// A broken symlink does not count — `is_file` follows links.
fn local_bin_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let bin_dir = dir.join("node_modules").join(".bin");
    // No `.exe` candidate: project `.bin` dirs never legitimately carry
    // one, and provider identity is derived from the extensionless
    // sibling script — a planted `.exe` would execute while a benign
    // script passed validation.
    let candidates: Vec<String> = if cfg!(windows) {
        vec![format!("{name}.cmd"), name.to_string()]
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

fn run_runtime_from_store(
    state_dir: &Path,
    name: &str,
    version_spec: &str,
    args: &[OsString],
) -> i32 {
    let result = crate::block_on_runtime(
        "pacquet-global-shim-runtime",
        materialize_runtime(state_dir, name.to_string(), version_spec.to_string()),
    );
    match result {
        Ok(bin) => exec_program(&bin, args),
        Err(error) => {
            eprintln!("pnpm: failed to prepare {name}@runtime:{version_spec}: {error:?}");
            1
        }
    }
}

/// Provision the package manager a project pins and run it.
///
/// The provisioning configuration is pnpm's own, anchored in its state
/// directory: the project decides *which* package manager runs, never
/// where its bytes come from.
fn run_package_manager_from_pin(
    state_dir: &Path,
    pm: PackageManager,
    version_spec: &str,
    name: &str,
    args: &[OsString],
) -> i32 {
    let spec = version_spec.to_string();
    let state_dir = state_dir.to_path_buf();
    let result = crate::block_on_runtime("pacquet-global-shim-pm", async move {
        let config = Config::leak(trusted_package_manager_config(&state_dir)?);
        provision::<SilentReporter>(config, pm, &spec).await
    });
    match result {
        Ok(engine) => {
            let program = engine.command(name);
            exec_program_with_bin_dirs(&program, &engine.bin_dirs, args)
        }
        Err(error) => {
            eprintln!("pnpm: failed to prepare {}@{version_spec}: {error:?}", pm.name());
            1
        }
    }
}

/// The configuration package-manager provisioning runs under: pnpm's own
/// trusted layers, anchored inside its state directory so no project's
/// `pnpm-workspace.yaml` can redirect the store the executable comes from.
fn trusted_package_manager_config(state_dir: &Path) -> miette::Result<Config> {
    if state_dir.as_os_str().is_empty() {
        return Err(miette::miette!("the pnpm state directory could not be resolved"));
    }
    trusted_runtime_config(&state_dir.join(PACKAGE_MANAGER_ENVS_DIR_NAME))
}

/// Run `program` with `bin_dirs` prepended to `PATH`. A JavaScript
/// package manager needs the Node.js it was provisioned with to be
/// reachable, and its own directory has to come first so a nested
/// invocation finds the same version.
fn exec_program_with_bin_dirs(program: &Path, bin_dirs: &[PathBuf], args: &[OsString]) -> i32 {
    match crate::path_env::prepend_dirs_to_path(bin_dirs) {
        // The `PATH` travels on the command rather than through this
        // process's own environment: an `exec` hands the child the
        // command's environment just the same, and nothing here has to
        // reason about which threads are running.
        Ok(path) => exec_program_with_path(program, args, Some(path.as_os_str())),
        Err(error) => {
            // Rendered as a report so the failure carries the same
            // `ERR_PNPM_BAD_PATH_DIR` code the commands report it under.
            eprintln!("pnpm: {:?}", miette::Report::new(error));
            1
        }
    }
}

/// Run `program` with `args`, replacing this process where the platform
/// allows. Exit codes follow the shell convention: 127 when the program
/// does not exist, 126 when it cannot be executed.
fn exec_program(program: &Path, args: &[OsString]) -> i32 {
    exec_program_with_path(program, args, None)
}

#[cfg(unix)]
fn exec_program_with_path(program: &Path, args: &[OsString], path: Option<&OsStr>) -> i32 {
    use std::os::unix::process::CommandExt as _;
    let mut command = Command::new(program);
    command.args(args);
    if let Some(path) = path {
        crate::path_env::set_command_path(&mut command, path);
    }
    let error = command.exec();
    eprintln!("pnpm: failed to exec {}: {error}", program.display());
    if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
}

#[cfg(windows)]
fn exec_program_with_path(program: &Path, args: &[OsString], path: Option<&OsStr>) -> i32 {
    // `.cmd`/`.bat` targets go to `Command::new` directly: the standard
    // library spawns them through `cmd.exe` itself with the
    // CVE-2024-24576 argument escaping, and rejects arguments it cannot
    // pass safely — a hand-rolled `cmd /c` would reintroduce that bug.
    let mut command = Command::new(program);
    command.args(args);
    if let Some(path) = path {
        crate::path_env::set_command_path(&mut command, path);
    }
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("pnpm: failed to run {}: {error}", program.display());
            if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
        }
    }
}

#[cfg(test)]
mod tests;
