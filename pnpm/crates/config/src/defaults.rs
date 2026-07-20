use crate::api::{EnvVar, GetCurrentDir, GetHomeDir};
use pacquet_store_dir::StoreDir;
use std::{env, path::PathBuf};

#[cfg(windows)]
use std::path::{Component, Path};

pub fn default_hoist_pattern() -> Vec<String> {
    vec!["*".to_string()]
}

/// Default for `git_shallow_hosts`, following
/// <https://github.com/npm/git/blob/1e1dbd26bd/lib/clone.js#L13-L19>.
#[must_use]
pub fn default_git_shallow_hosts() -> Vec<String> {
    vec![
        "github.com".to_string(),
        "gist.github.com".to_string(),
        "gitlab.com".to_string(),
        "bitbucket.com".to_string(),
        "bitbucket.org".to_string(),
    ]
}

/// Default for `public-hoist-pattern`: an empty list.
/// Writing a non-empty list on a fresh install would record a
/// `publicHoistPattern` in `.modules.yaml` that the next `pnpm`
/// invocation in the same project rejects with
/// `ERR_PNPM_PUBLIC_HOIST_PATTERN_DIFF`.
pub fn default_public_hoist_pattern() -> Vec<String> {
    Vec::new()
}

#[cfg(windows)]
fn get_drive_letter(current_dir: &Path) -> Option<char> {
    if let Some(Component::Prefix(prefix_component)) = current_dir.components().next()
        && let std::path::Prefix::Disk(disk_byte) | std::path::Prefix::VerbatimDisk(disk_byte) =
            prefix_component.kind()
    {
        return Some(disk_byte as char);
    }
    None
}

#[cfg(windows)]
fn default_store_dir_windows(home_dir: &Path, current_dir: &Path) -> PathBuf {
    let current_drive =
        get_drive_letter(current_dir).expect("current dir is an absolute path with drive letter");
    let home_drive =
        get_drive_letter(home_dir).expect("home dir is an absolute path with drive letter");

    if current_drive == home_drive {
        return home_dir.join("AppData/Local/pnpm/store");
    }

    PathBuf::from(format!(r"{current_drive}:\.pnpm-store"))
}

/// Generic over [`EnvVar`], [`GetHomeDir`], and [`GetCurrentDir`]
/// so unit tests can drive every branch — `PNPM_HOME` set,
/// `XDG_DATA_HOME` set, neither set — without mutating the process
/// environment. Production callers pass [`crate::Host`] for `Sys`,
/// which threads `home::home_dir` and `env::current_dir` through the
/// capability impls — see the `SmartDefault` expression on
/// [`crate::Config::store_dir`].
///
/// On non-Windows hosts, this is only the **initial** default. After
/// [`crate::Config::current`] has applied global config, workspace
/// yaml, and `PNPM_CONFIG_*` env vars, the store is re-resolved
/// against the project's volume via
/// [`crate::store_path::resolve_store_dir`] when none of those
/// sources pinned `storeDir`, falling back to `<mountpoint>/.pnpm-store`
/// when the home volume can't be hardlinked from the project. Without that
/// re-resolution a workspace on a separate case-sensitive volume
/// would land in the case-insensitive home store, breaking tools
/// that compare canonicalised file paths (typescript-eslint, for one).
pub fn default_store_dir<Sys>() -> StoreDir
where
    Sys: EnvVar + GetHomeDir + GetCurrentDir,
{
    // TODO: If env variables start with ~, make sure to resolve it into home_dir.
    if let Some(pnpm_home) = Sys::var("PNPM_HOME") {
        return PathBuf::from(pnpm_home).join("store").into();
    }

    if let Some(xdg_data_home) = Sys::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg_data_home).join("pnpm").join("store").into();
    }

    // Using ~ (tilde) for defining home path is not supported in Rust and
    // needs to be resolved into an absolute path.
    let home_dir = Sys::home_dir().expect("Home directory is not available");

    #[cfg(windows)]
    if cfg!(windows) {
        let current_dir = Sys::current_dir().expect("current directory is unavailable");
        return default_store_dir_windows(&home_dir, &current_dir).into();
    }

    // <https://doc.rust-lang.org/std/env/consts/constant.OS.html>
    match env::consts::OS {
        "linux" => home_dir.join(".local/share/pnpm/store").into(),
        "macos" => home_dir.join("Library/pnpm/store").into(),
        _ => panic!("unsupported operating system: {}", env::consts::OS),
    }
}

/// The directory-layout version for global installs, appended to the
/// global packages root. Bumping it isolates a new pnpm major's global
/// packages from older ones.
pub const GLOBAL_LAYOUT_VERSION: &str = "v11";

/// Resolve pnpm's home (data) directory, the root under which global
/// packages and global bins live.
///
/// Resolution order: `PNPM_HOME` → `XDG_DATA_HOME/pnpm` → `~/Library/pnpm`
/// (macOS) / `~/.local/share/pnpm` (non-Windows) / `%LOCALAPPDATA%/pnpm`
/// (Windows) → `~/.pnpm`. Returns `None` only when the home directory
/// cannot be determined and no env override is set.
#[must_use]
pub fn default_pnpm_home_dir<Sys>() -> Option<PathBuf>
where
    Sys: EnvVar + GetHomeDir,
{
    if let Some(pnpm_home) = Sys::var("PNPM_HOME") {
        return Some(PathBuf::from(pnpm_home));
    }
    if let Some(xdg_data_home) = Sys::var("XDG_DATA_HOME") {
        return Some(PathBuf::from(xdg_data_home).join("pnpm"));
    }
    let home_dir = Sys::home_dir()?;
    Some(match env::consts::OS {
        "macos" => home_dir.join("Library/pnpm"),
        "windows" => Sys::var("LOCALAPPDATA")
            .map_or_else(|| home_dir.join(".pnpm"), |local| PathBuf::from(local).join("pnpm")),
        // pnpm treats every non-Windows platform as Unix here.
        _ => home_dir.join(".local/share/pnpm"),
    })
}

pub fn default_modules_dir() -> PathBuf {
    // TODO: find directory with package.json
    env::current_dir().expect("current directory is unavailable").join("node_modules")
}

/// Resolve the directory pnpm reads `config.yaml` (the global config
/// file) from. Threads this crate's [`EnvVar`] / [`GetHomeDir`] seam
/// into [`pacquet_config_dir::config_dir`] — the shared resolver, also
/// used by the registry server — under the `pnpm` leaf.
pub fn default_config_dir<Sys>() -> Option<PathBuf>
where
    Sys: EnvVar + GetHomeDir,
{
    let xdg_config_home = Sys::var("XDG_CONFIG_HOME");
    let local_app_data = Sys::var("LOCALAPPDATA");
    pacquet_config_dir::config_dir(
        "pnpm",
        env::consts::OS,
        xdg_config_home.as_deref(),
        local_app_data.as_deref(),
        Sys::home_dir,
    )
}

/// Resolve the default packument-cache directory.
///
/// Generic over [`EnvVar`] and [`GetHomeDir`] for the same reason
/// as [`default_store_dir`]: unit tests drive every branch without
/// mutating the process environment. Production callers pass
/// [`crate::Host`] for `Sys`, which threads `home::home_dir` through
/// the [`GetHomeDir`] impl.
pub fn default_cache_dir<Sys>() -> PathBuf
where
    Sys: EnvVar + GetHomeDir,
{
    if let Some(xdg_cache_home) = Sys::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg_cache_home).join("pnpm");
    }
    let home_dir = Sys::home_dir().expect("Home directory is not available");
    match env::consts::OS {
        "macos" => home_dir.join("Library/Caches/pnpm"),
        "windows" => Sys::var("LOCALAPPDATA").map_or_else(
            || home_dir.join(".pnpm-cache"),
            |local_app_data| PathBuf::from(local_app_data).join("pnpm-cache"),
        ),
        _ => home_dir.join(".cache/pnpm"),
    }
}

pub fn default_virtual_store_dir() -> PathBuf {
    // TODO: find directory with package.json
    env::current_dir().expect("current directory is unavailable").join("node_modules/.pnpm")
}

/// Default for `enableGlobalVirtualStore`: `false` for regular installs.
///
/// It is only enabled by default on the `pnpm install --global` path.
/// Pacquet doesn't have a `--global` CLI flag at all (only
/// `install --frozen-lockfile`), so the only applicable default is the
/// `false` one.
pub fn default_enable_global_virtual_store() -> bool {
    false
}

pub fn default_registry() -> String {
    "https://registry.npmjs.org/".to_string()
}

pub fn default_modules_cache_max_age() -> u64 {
    10080
}

/// Default `virtualStoreDirMaxLength`: platform-aware (60 on Windows,
/// 120 elsewhere).
#[must_use]
pub fn default_virtual_store_dir_max_length() -> u64 {
    if cfg!(windows) { 60 } else { 120 }
}

/// Default `peersSuffixMaxLength`: the fallback used when computing the
/// peer-dependency graph hash.
///
/// Kept as a free function (not a re-export of
/// `pacquet_lockfile::DEFAULT_PEERS_SUFFIX_MAX_LENGTH`) so
/// `pacquet-config` doesn't pull in the lockfile crate just for one
/// integer. Both copies must agree.
#[must_use]
pub fn default_peers_suffix_max_length() -> u64 {
    1000
}

pub fn default_fetch_retries() -> u32 {
    2
}

pub fn default_fetch_retry_factor() -> u32 {
    10
}

pub fn default_fetch_retry_mintimeout() -> u64 {
    10_000
}

pub fn default_fetch_retry_maxtimeout() -> u64 {
    60_000
}

/// The CLI's user-facing release version — the same value
/// `pnpm --version` prints. Single source of truth so the CLI
/// version string and the default `User-Agent` (`default_user_agent`)
/// can't drift apart. `pnpm bump` keeps this constant in sync with the
/// version of the npm wrapper package (`pnpm/npm/pnpm/package.json`);
/// the release workflow verifies the two match before building.
pub const PNPM_VERSION: &str = "12.0.0-alpha.17";

pub fn default_fetch_timeout() -> u64 {
    pacquet_network::DEFAULT_FETCH_TIMEOUT_MS
}

/// Default `User-Agent`, in the format
/// `${name}/${version} npm/? node/${nodeVersion} ${platform} ${arch}`.
/// The `name/version` segment is `pnpm/<version>`. There is no embedded
/// Node runtime, so the `node/` segment is the same `?` placeholder used
/// for `npm/`. Platform and arch use Node's naming via
/// [`pacquet_detect_libc::host_platform`] / [`pacquet_detect_libc::host_arch`].
pub fn default_user_agent() -> String {
    format!(
        "pnpm/{PNPM_VERSION} npm/? node/? {} {}",
        pacquet_detect_libc::host_platform(),
        pacquet_detect_libc::host_arch(),
    )
}

/// Default `childConcurrency`: `min(4, availableParallelism())`. Read at
/// runtime so `cargo test` and overrides via yaml still resolve to a
/// usable value on 1-core sandboxes.
pub fn default_child_concurrency() -> u32 {
    default_child_concurrency_with_parallelism(available_parallelism())
}

/// Internal helper exposed for tests so they can pin the
/// `parallelism` input directly rather than reading it from the host.
pub fn default_child_concurrency_with_parallelism(parallelism: u32) -> u32 {
    parallelism.min(4)
}

/// Default `workspaceConcurrency`, the default for `workspace-concurrency`.
///
/// Identical in value to `default_child_concurrency` — both settings
/// resolve through the same default-concurrency formula — but exposed
/// under its own name so the [`crate::Config::workspace_concurrency`]
/// field default reads at its own call site.
#[must_use]
pub fn default_workspace_concurrency() -> u32 {
    default_child_concurrency()
}

/// Available CPU parallelism. Floors at 1.
#[must_use]
pub fn available_parallelism() -> u32 {
    std::thread::available_parallelism().map_or(1, |count| count.get() as u32).max(1)
}

/// Resolve `childConcurrency` from a possibly-negative yaml value
/// to a concrete `u32`.
///
/// The negative-offset semantics let users say "use all cores minus
/// N" without hardcoding the core count.
#[must_use]
pub fn resolve_child_concurrency(option: Option<i32>) -> u32 {
    resolve_child_concurrency_with_parallelism(option, available_parallelism())
}

/// Internal helper exposed for tests so they can pin the
/// `parallelism` input — the resolver logic itself, with the
/// parallelism input injected rather than read from the OS.
pub fn resolve_child_concurrency_with_parallelism(option: Option<i32>, parallelism: u32) -> u32 {
    match option {
        None => default_child_concurrency_with_parallelism(parallelism),
        Some(n) if n > 0 => n as u32,
        // `unsigned_abs` instead of `(-n) as u32` — the latter
        // panics in debug builds on `n == i32::MIN` (negation
        // overflow); the former returns `i32::MAX as u32 + 1`
        // safely.
        Some(n) => parallelism.saturating_sub(n.unsigned_abs()).max(1),
    }
}

/// Default `unsafePerm`: `true` on Windows and Cygwin, and on POSIX
/// whenever the process is not running as root (uid != 0).
///
/// Pacquet's executor doesn't currently consume `unsafe_perm` to
/// actually drop uid/gid, but the TMPDIR-isolation side of the flag is
/// honored — see `pacquet_executor::make_env`.
///
/// Cygwin needs explicit handling because Rust's
/// [`x86_64-pc-cygwin` target](https://doc.rust-lang.org/rustc/platform-support/x86_64-pc-cygwin.html)
/// emits `target_os = "cygwin"` with `cfg!(unix)` set and
/// `cfg!(windows)` *unset*, so a plain `cfg!(windows)` check would
/// fall through to the uid logic and diverge from the unconditional-true
/// Cygwin behavior.
#[must_use]
pub fn default_unsafe_perm() -> bool {
    platform_unsafe_perm_default()
}

#[cfg(any(windows, target_os = "cygwin"))]
fn platform_unsafe_perm_default() -> bool {
    true
}

#[cfg(all(unix, not(target_os = "cygwin")))]
fn platform_unsafe_perm_default() -> bool {
    is_unsafe_perm_posix(posix_getuid())
}

/// Targets that are neither Windows, Cygwin, nor POSIX
/// (`wasm32-*`, `redox`, etc.) have no `getuid()` and no privilege
/// model to drop into. Default to `true` so lifecycle scripts
/// behave the same as on Windows.
#[cfg(not(any(windows, unix)))]
fn platform_unsafe_perm_default() -> bool {
    true
}

/// Pure-logic helper exposed for tests so the POSIX branch can be
/// exercised under both root and non-root uids without root
/// privileges. Mirrors the POSIX half of [`default_unsafe_perm`].
#[must_use]
pub fn is_unsafe_perm_posix(uid: u32) -> bool {
    // `unsafe_perm = true` means "do NOT drop privileges". Drop
    // only when we *are* root (uid == 0).
    uid != 0
}

/// Safe wrapper around `libc::getuid` — contains the `unsafe`
/// FFI block internally so the caller doesn't need to propagate
/// `unsafe`. `libc::getuid` is documented as always-safe: it
/// reads a kernel field, has no side effects, and cannot fail.
/// Only compiled on POSIX-excluding-Cygwin since that's the only
/// branch that actually calls it.
#[cfg(all(unix, not(target_os = "cygwin")))]
fn posix_getuid() -> u32 {
    // SAFETY: `libc::getuid` has no preconditions; it reads a
    // kernel-owned uid field and cannot fail.
    unsafe { libc::getuid() as u32 }
}

#[cfg(test)]
mod tests;
