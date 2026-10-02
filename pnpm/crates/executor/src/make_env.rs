use crate::lifecycle::{DEV_PREINSTALL_ALREADY_RAN_ENV, ROOT_PREINSTALL_ALREADY_RAN_ENV};
use serde_json::Value;
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// Env var pnpm stamps as `false` into every spawned script and exec
/// child so a nested `pnpm run` / `pnpm exec` skips the
/// verify-deps-before-run check; the config layer reads it back with
/// priority over every other source of that setting (pnpm/pnpm#10060).
pub const VERIFY_DEPS_BEFORE_RUN_ENV: &str = "pnpm_config_verify_deps_before_run";

/// Inputs needed to build the env for a single lifecycle hook spawn:
/// the package context, the per-call stamps the hook runner adds, and
/// the caller-supplied `extra_env`. `extra_env` carries the user's
/// `updateConfig` `extraEnv` plus any pnpm-controlled keys the caller
/// merges in first (e.g. `NODE_OPTIONS` from `nodeOptions`); the
/// reserved per-call stamps below override all of it regardless of
/// origin — see [`build_env`].
pub struct EnvOptions<'a> {
    pub stage: &'a str,
    pub script: &'a str,
    pub pkg_root: &'a Path,
    pub script_src_dir: &'a Path,
    pub unsafe_perm: bool,
    pub environment: crate::ScriptEnvironment<'a>,
}

/// The product of [`build_env`]: a ready-to-spawn env map and the
/// `TMPDIR` the caller must create when `unsafe_perm` is false (so
/// the side effect stays out of this pure builder).
pub struct EnvBuild {
    pub env: HashMap<String, String>,
    pub tmpdir: Option<PathBuf>,
}

/// Build the env for a lifecycle script spawn, given the parent
/// process env to inherit from.
///
/// Two layers of work: the base env build (parent-env filter,
/// `npm_package_*` recursion, multi-line escaping), then the
/// caller-supplied `extra_env`, then the reserved per-call stamps
/// (`INIT_CWD`, `PNPM_SCRIPT_SRC_DIR`, `npm_config_user_agent`, the
/// verify-deps guard, `npm_lifecycle_script`) applied last so they win
/// over anything `extra_env` set — matching TS `runLifecycleHook`.
///
/// `parent_env` is taken by value so the production caller can pass
/// `env::vars().collect()` and tests can pass a controlled fixture
/// without racing on the global process env.
#[must_use]
pub fn build_env(
    opts: &EnvOptions<'_>,
    manifest: &Value,
    parent_env: HashMap<String, String>,
) -> EnvBuild {
    build_env_for_platform(opts, manifest, parent_env, cfg!(windows))
}

fn build_env_for_platform(
    opts: &EnvOptions<'_>,
    manifest: &Value,
    parent_env: HashMap<String, String>,
    is_windows: bool,
) -> EnvBuild {
    // 1. User-defined `npm_config_*` such as `npm_config_platform_arch`
    //    are preserved. `pnpm_*` keys such as `PNPM_HOME` are
    //    intentionally NOT in the filter.
    let mut env = filter_parent_env(parent_env, is_windows);

    // 2. `npm_package_*` recursive stamp.
    stamp_package(&mut env, "npm_package_", manifest);

    // 3. Per-call stamping.
    env.insert("npm_lifecycle_event".into(), opts.stage.to_string());

    stamp_executables(&mut env, &opts.environment, opts.pkg_root, is_windows);

    // 4. `extra_env` (the user's `updateConfig` `extraEnv` plus any
    //    pnpm-controlled keys the caller merged in, such as
    //    `NODE_OPTIONS`) is applied BEFORE the reserved per-call stamps
    //    below, so pnpm's own stamps win on conflict.
    //    `extra_env` is also the one route by which a delegation marker
    //    ([`DEV_PREINSTALL_ALREADY_RAN_ENV`] or
    //    [`ROOT_PREINSTALL_ALREADY_RAN_ENV`]) could re-enter after
    //    [`filter_parent_env`] dropped it, so it is refused here — under
    //    the same casing rule that filter uses, since on Windows a
    //    differently-cased entry names the same variable.
    //    On Windows an `extra_env` key also replaces every differently
    //    cased spelling already present, such as the stamped
    //    `npm_config_node_gyp` default, which would otherwise race it at
    //    spawn time.
    for (k, v) in opts.environment.extra_env {
        if is_delegation_marker(k, is_windows) {
            continue;
        }
        if is_windows {
            env.retain(|key, _| !key.eq_ignore_ascii_case(k));
        }
        env.insert(k.clone(), v.clone());
    }

    env.insert("INIT_CWD".into(), opts.environment.init_cwd.to_string_lossy().into_owned());
    env.insert("PNPM_SCRIPT_SRC_DIR".into(), opts.script_src_dir.to_string_lossy().into_owned());

    if let Some(ua) = opts.environment.user_agent {
        env.insert("npm_config_user_agent".into(), ua.to_string());
    }

    // Breaks the recursion a spawned install's lifecycle scripts would
    // otherwise enter. Stamped after `extra_env` so a user `extraEnv`
    // can't disable the guard (pnpm keeps this key authoritative).
    env.insert(VERIFY_DEPS_BEFORE_RUN_ENV.into(), "false".into());

    // 5. TMPDIR under <wd>/node_modules/.tmp when !unsafe_perm.
    let tmpdir = if opts.unsafe_perm {
        None
    } else {
        let dir = opts.pkg_root.join("node_modules").join(".tmp");
        // Windows treats differently cased spellings as one variable,
        // so remove them before inserting the authoritative override.
        if is_windows {
            env.retain(|key, _| !key.eq_ignore_ascii_case("TMPDIR"));
        }
        env.insert("TMPDIR".into(), dir.to_string_lossy().into_owned());
        Some(dir)
    };

    // 6. `npm_lifecycle_script` is set after `extra_env`, so the
    //    caller can never clobber it.
    env.insert("npm_lifecycle_script".into(), opts.script.to_string());

    EnvBuild { env, tmpdir }
}

/// Keep PATH (handled by the caller) and every key [`is_stamping_key`]
/// does not claim.
///
/// On Windows the comparison is case-insensitive because Rust's
/// `Command::env` treats env keys case-insensitively on that
/// platform (see [`Command::env`] docs). Leaving e.g. `NPM_PACKAGE_FOO`
/// alongside our stamped inserts would collapse at spawn time with
/// an unpredictable winner.
///
/// [`Command::env`]: https://doc.rust-lang.org/std/process/struct.Command.html#method.env
fn filter_parent_env(env: HashMap<String, String>, is_windows: bool) -> HashMap<String, String> {
    env.into_iter()
        .filter(|(k, _)| !is_stamping_key(k, is_windows))
        .collect()
}

/// Whether `key` must be dropped from the inherited parent env: an
/// `npm_package_*` stamp, a `(npm|pnpm)_config_*` auth credential, a
/// per-call stamp [`build_env`] re-derives (`NODE`, `INIT_CWD`,
/// `PNPM_SCRIPT_SRC_DIR`), or a delegation marker
/// ([`DEV_PREINSTALL_ALREADY_RAN_ENV`], [`ROOT_PREINSTALL_ALREADY_RAN_ENV`]).
/// Stripping the auth credentials keeps them out of dependency lifecycle
/// scripts; stripping the delegation markers keeps them scoped to the
/// install that received them, so a nested install started by a script
/// still runs its own hooks.
///
/// `is_windows` toggles case-insensitive matching so test code can
/// drive both branches without `#[cfg(windows)]` gating the test
/// bodies. Production callers pass `cfg!(windows)`.
fn is_stamping_key(key: &str, is_windows: bool) -> bool {
    if strip_env_prefix(key, "npm_package_", is_windows).is_some() {
        return true;
    }
    if let Some(rest) = strip_env_prefix(key, "npm_config_", is_windows)
        .or_else(|| strip_env_prefix(key, "pnpm_config_", is_windows))
        && (rest.starts_with(['_', '/', '@']) || rest.contains(":_"))
    {
        return true;
    }
    const DROPPED: [&str; 5] = [
        "NODE",
        "INIT_CWD",
        "PNPM_SCRIPT_SRC_DIR",
        DEV_PREINSTALL_ALREADY_RAN_ENV,
        ROOT_PREINSTALL_ALREADY_RAN_ENV,
    ];
    if is_windows {
        return DROPPED
            .iter()
            .any(|name| key.eq_ignore_ascii_case(name));
    }
    DROPPED.contains(&key)
}

/// Whether `key` names [`DEV_PREINSTALL_ALREADY_RAN_ENV`] or
/// [`ROOT_PREINSTALL_ALREADY_RAN_ENV`], under the same casing rule
/// [`is_stamping_key`] applies: on Windows every spelling is the same
/// variable, so every spelling must be dropped.
fn is_delegation_marker(key: &str, is_windows: bool) -> bool {
    const MARKERS: [&str; 2] = [DEV_PREINSTALL_ALREADY_RAN_ENV, ROOT_PREINSTALL_ALREADY_RAN_ENV];
    if is_windows {
        return MARKERS
            .iter()
            .any(|name| key.eq_ignore_ascii_case(name));
    }
    MARKERS.contains(&key)
}

/// Return the slice of `key` after `prefix` when `key` starts with it
/// — case-sensitively on POSIX, case-insensitively on Windows (where
/// `Command::env` collapses key case). Returns `None` otherwise.
///
/// The Windows branch compares bytes rather than chars so it never
/// panics on a non-ASCII key whose UTF-8 representation crosses the
/// prefix boundary; `prefix` is ASCII, so `prefix.len()` is a valid
/// char boundary whenever the bytes match.
fn strip_env_prefix<'key>(key: &'key str, prefix: &str, is_windows: bool) -> Option<&'key str> {
    if is_windows {
        if key
            .as_bytes()
            .get(..prefix.len())
            .is_some_and(|b| b.eq_ignore_ascii_case(prefix.as_bytes()))
        {
            return key.get(prefix.len()..);
        }
        return None;
    }
    key.strip_prefix(prefix)
}

/// Whether `key` names the `PATH` variable. Windows compares environment
/// names case-insensitively, and its system variable is typically `Path`.
/// Elsewhere names are case-sensitive, so a `Path` variable is a variable
/// of its own and must not stand in for `PATH`.
pub(crate) fn is_path_key(key: &str) -> bool {
    if cfg!(windows) { key.eq_ignore_ascii_case("PATH") } else { key == "PATH" }
}

/// Look up the `PATH` value from `env`, spelled as [`is_path_key`]
/// accepts it, so the rest of [`build_env`] stays independent of casing.
pub(crate) fn path_value(env: &HashMap<String, String>) -> Option<String> {
    env.iter()
        .find_map(|(k, v)| is_path_key(k).then(|| v.clone()))
}

/// Look up `node` along the supplied `PATH`. Driven by the filtered
/// `parent_env`'s PATH (not the process-global env) so [`build_env`]
/// stays deterministic given its inputs — matching the docstring
/// contract.
fn find_node_in_path(path: Option<&OsStr>) -> Option<PathBuf> {
    let path = path?;
    let node_name = if cfg!(windows) { "node.exe" } else { "node" };
    pnpm_fs::split_paths(path)
        .find_map(|dir| {
            let candidate = dir.join(node_name);
            candidate.is_file().then_some(candidate)
        })
}

/// Recursively stamp `npm_package_*` env vars from the manifest. JSON
/// arrays iterate as indexed keys; objects iterate as named keys. The
/// top-level call uses prefix `npm_package_`; recursion appends
/// `<sanitized-key>_`.
fn stamp_package(env: &mut HashMap<String, String>, prefix: &str, value: &Value) {
    let pairs: Vec<(String, &Value)> = match value {
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| (k.clone(), v))
            .collect(),
        Value::Array(arr) => arr
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => return,
    };

    for (key, value) in pairs {
        if !stamps_manifest_field(prefix, &key) {
            continue;
        }
        let env_key = sanitize_env_key(&format!("{prefix}{key}"));
        match value {
            Value::Object(_) | Value::Array(_) => {
                stamp_package(env, &format!("{env_key}_"), value);
            }
            Value::String(text) => {
                env.insert(env_key, escape_newlines(text));
            }
            Value::Number(number) => {
                env.insert(env_key, number.to_string());
            }
            Value::Bool(flag) => {
                env.insert(env_key, flag.to_string());
            }
            Value::Null => {
                env.insert(env_key, String::new());
            }
        }
    }
}

/// Stamp the executables a script resolves through: the Node binary, the
/// package manifest, pnpm itself, and the `node-gyp` default.
///
/// `npm_config_node_gyp` is a default pnpm supplies, not a reserved stamp: TS
/// `npm-lifecycle` sets it before spreading `extraEnv`, so a user `extraEnv`
/// overrides it, and only when the environment left it unset, so an inherited
/// value wins too. It is stamped before `extra_env` to match.
fn stamp_executables(
    env: &mut HashMap<String, String>,
    opts: &crate::ScriptEnvironment<'_>,
    pkg_root: &Path,
    is_windows: bool,
) {
    let parent_path = path_value(env);
    env.extend(
        package_manager_env(
            opts.init_cwd,
            opts.node_execpath,
            opts.npm_execpath,
            parent_path.as_deref().map(OsStr::new),
        )
        .into_iter()
        .map(|(key, value)| (key, value.to_string_lossy().into_owned())),
    );

    env.insert(
        "npm_package_json".into(),
        pkg_root
            .join("package.json")
            .to_string_lossy()
            .into_owned(),
    );

    // A user's own `npm_config_node_gyp`, inherited from the parent
    // environment, wins: TS `npm-lifecycle` fills the variable only when
    // the environment left it unset, and reads an empty value as unset.
    // Windows treats differently cased spellings as one variable, so an
    // inherited `NPM_CONFIG_NODE_GYP` must win there too, and an empty
    // spelling must be removed before the default is stamped or the two
    // would race in the child's environment block.
    const NODE_GYP_KEY: &str = "npm_config_node_gyp";
    let names_node_gyp = |key: &str| {
        if is_windows { key.eq_ignore_ascii_case(NODE_GYP_KEY) } else { key == NODE_GYP_KEY }
    };
    let inherited = env
        .iter()
        .any(|(key, value)| names_node_gyp(key) && !value.is_empty());
    if let Some(path) = opts.node_gyp_path
        && !inherited
    {
        env.retain(|key, _| !names_node_gyp(key));
        env.insert(NODE_GYP_KEY.into(), path.to_string_lossy().into_owned());
    }
}

/// Environment identifying the invoking package manager and working directory.
#[must_use]
pub fn package_manager_env(
    init_cwd: &Path,
    node_execpath: Option<&Path>,
    npm_execpath: Option<&Path>,
    path: Option<&OsStr>,
) -> HashMap<String, OsString> {
    let mut env = HashMap::new();
    env.insert("INIT_CWD".into(), init_cwd.as_os_str().to_os_string());
    let node_execpath = node_execpath.map(Path::to_path_buf).or_else(|| find_node_in_path(path));
    if let Some(node) = node_execpath {
        let node_path = node.into_os_string();
        env.insert("npm_node_execpath".into(), node_path.clone());
        env.insert("NODE".into(), node_path);
    }
    let npm_execpath = npm_execpath
        .map(Path::to_path_buf)
        .or_else(|| crate::current_pnpm_exe().ok());
    if let Some(path) = npm_execpath {
        env.insert("npm_execpath".into(), path.into_os_string());
    }
    env
}

/// Whether one manifest field reaches the environment. The top level keeps
/// only `name`, `version`, `config`, `engines` and `bin`; below those,
/// recursion keeps everything. An underscore-prefixed key is npm's own
/// bookkeeping and never stamped.
fn stamps_manifest_field(prefix: &str, key: &str) -> bool {
    if key.starts_with('_') {
        return false;
    }
    matches!(key, "name" | "version" | "config" | "engines" | "bin")
        || prefix.starts_with("npm_package_config_")
        || prefix.starts_with("npm_package_engines_")
        || prefix.starts_with("npm_package_bin_")
}

/// Replace every character that is not `[a-zA-Z0-9_]` with `_`, the
/// sanitization an env key derived from a manifest field needs.
fn sanitize_env_key(raw: &str) -> String {
    raw.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '_' { ch } else { '_' })
        .collect()
}

/// JSON-encode multi-line strings (those containing `\n`) so child
/// shells don't break on embedded newlines; single-line strings pass
/// through unchanged.
fn escape_newlines(text: &str) -> String {
    if text.contains('\n') { Value::String(text.to_string()).to_string() } else { text.to_string() }
}

#[cfg(test)]
mod tests;
