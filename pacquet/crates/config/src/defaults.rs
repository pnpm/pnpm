use pacquet_store_dir::StoreDir;
use std::{env, path::PathBuf};

#[cfg(windows)]
use std::{path::Component, path::Path};

pub fn default_hoist_pattern() -> Vec<String> {
    vec!["*".to_string()]
}

/// Default for `git_shallow_hosts`. Mirrors pnpm v11's default at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L155-L162>,
/// which follows
/// <https://github.com/npm/git/blob/1e1dbd26bd/lib/clone.js#L13-L19>.
pub fn default_git_shallow_hosts() -> Vec<String> {
    vec![
        "github.com".to_string(),
        "gist.github.com".to_string(),
        "gitlab.com".to_string(),
        "bitbucket.com".to_string(),
        "bitbucket.org".to_string(),
    ]
}

pub fn default_public_hoist_pattern() -> Vec<String> {
    vec!["*eslint*".to_string(), "*prettier*".to_string()]
}

// Get the drive letter from a path on Windows. If it's not a Windows path, return None.
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

    PathBuf::from(format!("{current_drive}:\\.pnpm-store"))
}

/// If the $PNPM_HOME env variable is set, then $PNPM_HOME/store
/// If the $XDG_DATA_HOME env variable is set, then $XDG_DATA_HOME/pnpm/store
/// On Windows: ~/AppData/Local/pnpm/store
/// On macOS: ~/Library/pnpm/store
/// On Linux: ~/.local/share/pnpm/store
pub fn default_store_dir() -> StoreDir {
    // TODO: If env variables start with ~, make sure to resolve it into home_dir.
    if let Ok(pnpm_home) = env::var("PNPM_HOME") {
        return PathBuf::from(pnpm_home).join("store").into();
    }

    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg_data_home).join("pnpm").join("store").into();
    }

    // Using ~ (tilde) for defining home path is not supported in Rust and
    // needs to be resolved into an absolute path.
    let home_dir = home::home_dir().expect("Home directory is not available");

    #[cfg(windows)]
    if cfg!(windows) {
        let current_dir = env::current_dir().expect("current directory is unavailable");
        return default_store_dir_windows(&home_dir, &current_dir).into();
    }

    // https://doc.rust-lang.org/std/env/consts/constant.OS.html
    match env::consts::OS {
        "linux" => home_dir.join(".local/share/pnpm/store").into(),
        "macos" => home_dir.join("Library/pnpm/store").into(),
        _ => panic!("unsupported operating system: {}", env::consts::OS),
    }
}

pub fn default_modules_dir() -> PathBuf {
    // TODO: find directory with package.json
    env::current_dir().expect("current directory is unavailable").join("node_modules")
}

pub fn default_virtual_store_dir() -> PathBuf {
    // TODO: find directory with package.json
    env::current_dir().expect("current directory is unavailable").join("node_modules/.pnpm")
}

/// Default for `enableGlobalVirtualStore`. Matches pnpm v11's
/// effective default for regular installs: `false`.
///
/// The `true` assignment at
/// [`config/reader/src/index.ts:392-394`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L392-L394)
/// lives entirely inside the
/// [`if (cliOptions['global'])` block](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts#L348-L395)
/// (the `pnpm install --global` path), surrounded by
/// `CONFIG_CONFLICT_*_WITH_GLOBAL` errors and closed by
/// `} else if (!pnpmConfig.bin)`. For regular `pnpm install` the
/// value stays `null`/unset, which evaluates as `false` everywhere
/// downstream. Pacquet doesn't have a `--global` CLI flag at all
/// (only `install --frozen-lockfile`), so the only applicable
/// upstream default is the `false` one.
///
/// pnpm/pacquet#444 originally cited the same `L392-L394` range but
/// read it as an unconditional default — corrected here.
pub fn default_enable_global_virtual_store() -> bool {
    false
}

pub fn default_registry() -> String {
    "https://registry.npmjs.org/".to_string()
}

pub fn default_modules_cache_max_age() -> u64 {
    10080
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

/// Default `childConcurrency` matching upstream's
/// [`getDefaultWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L21-L23):
/// `min(4, availableParallelism())`. Read at runtime so `cargo test`
/// and overrides via yaml still resolve to a usable value on
/// 1-core sandboxes.
pub fn default_child_concurrency() -> u32 {
    default_child_concurrency_with_parallelism(available_parallelism())
}

/// Internal helper exposed for tests so they can pin the
/// `parallelism` input. Upstream's test suite mocks
/// `os.availableParallelism` via Jest; pacquet injects the value
/// directly. Mirrors upstream's [`getDefaultWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L21-L23).
pub fn default_child_concurrency_with_parallelism(parallelism: u32) -> u32 {
    parallelism.min(4)
}

/// Available CPU parallelism, mirroring upstream's
/// [`getAvailableParallelism`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L5-L13).
/// Floors at 1.
pub fn available_parallelism() -> u32 {
    std::thread::available_parallelism().map(|count| count.get() as u32).unwrap_or(1).max(1)
}

/// Resolve `childConcurrency` from a possibly-negative yaml value
/// to a concrete `u32`. Mirrors upstream's
/// [`getWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L25-L34):
///
/// - `None` → default (`min(4, parallelism)`).
/// - Positive `n` → `n`.
/// - Zero or negative `n` → `max(1, parallelism - |n|)`.
///
/// The negative-offset semantics let users say "use all cores minus
/// N" without hardcoding the core count.
pub fn resolve_child_concurrency(option: Option<i32>) -> u32 {
    resolve_child_concurrency_with_parallelism(option, available_parallelism())
}

/// Internal helper exposed for tests so they can pin the
/// `parallelism` input. Mirrors upstream's
/// [`getWorkspaceConcurrency`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/concurrency.ts#L25-L34)
/// — the resolver logic itself, with the parallelism input
/// injected rather than read from the OS.
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

/// Default `unsafePerm` matching upstream's
/// [`extendBuildOptions`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/extendBuildOptions.ts#L83-L86):
///
/// ```ts
/// unsafePerm: process.platform === 'win32' ||
///   process.platform === 'cygwin' ||
///   !process.setgid ||
///   process.getuid?.() !== 0,
/// ```
///
/// Truth table:
/// - Windows or Cygwin → `true`. POSIX privilege drop doesn't
///   apply; upstream's `process.platform === 'win32' ||
///   process.platform === 'cygwin'` branch fires unconditionally.
/// - POSIX (excluding Cygwin), not running as root → `true`. Nothing
///   to drop from.
/// - POSIX, running as root → `false`. Lifecycle scripts will run
///   under TMPDIR isolation to `node_modules/.tmp`.
/// - Anything else (e.g. `wasm32-*`) → `true`. No POSIX privilege
///   model to drop into; behave like upstream's Windows branch.
///
/// Pacquet's executor doesn't currently consume `unsafe_perm` to
/// actually drop uid/gid (upstream's own [`@pnpm/npm-lifecycle`
/// implementation](https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/index.js#L236-L239)
/// is a no-op in practice because `opts.user` / `opts.group` are
/// never populated), but the TMPDIR-isolation side of the flag is
/// honored — see `pacquet_executor::make_env`.
///
/// Cygwin needs explicit handling because Rust's
/// [`x86_64-pc-cygwin` target](https://doc.rust-lang.org/rustc/platform-support/x86_64-pc-cygwin.html)
/// emits `target_os = "cygwin"` with `cfg!(unix)` set and
/// `cfg!(windows)` *unset*, so a plain `cfg!(windows)` check would
/// fall through to the uid logic and diverge from upstream's
/// unconditional-true Cygwin behavior. The `!process.setgid` branch
/// in upstream is a Node-version compatibility check for older Node
/// where `setgid` doesn't exist; it doesn't translate to Rust
/// (libc's `setgid` is always available on POSIX hosts where libc
/// compiles).
pub fn default_unsafe_perm() -> bool {
    platform_unsafe_perm_default()
}

/// Windows / Cygwin branch — always `true` (no POSIX privilege
/// drop applies).
#[cfg(any(windows, target_os = "cygwin"))]
fn platform_unsafe_perm_default() -> bool {
    true
}

/// POSIX (excluding Cygwin) — drop privileges only when running
/// as root.
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
