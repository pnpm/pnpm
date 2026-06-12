use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
};

/// Inputs needed to build the env for a single lifecycle hook spawn.
///
/// Mirrors the union of `makeEnv` inputs and the per-call additions in
/// `lifecycle()` from `@pnpm/npm-lifecycle@d2d8e790` at
/// <https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/index.js#L52-L113>
/// plus the wrapper's `extraEnv` additions at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/exec/lifecycle/src/runLifecycleHook.ts#L119-L124>.
pub struct EnvOptions<'a> {
    pub stage: &'a str,
    pub script: &'a str,
    pub pkg_root: &'a Path,
    pub init_cwd: &'a Path,
    pub script_src_dir: &'a Path,
    pub node_execpath: Option<&'a Path>,
    pub npm_execpath: Option<&'a Path>,
    pub node_gyp_path: Option<&'a Path>,
    pub user_agent: Option<&'a str>,
    pub unsafe_perm: bool,
    pub extra_env: &'a HashMap<String, String>,
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
/// Ports `makeEnv` + the surrounding env block in `lifecycle()` from
/// `@pnpm/npm-lifecycle@d2d8e790`:
/// - `index.js:73-104` for the post-`makeEnv` stamping.
/// - `index.js:354-414` for `makeEnv` itself (parent-env filter,
///   `npm_package_*` recursion, multi-line escaping).
///
/// Plus the wrapper's `extraEnv` additions at
/// `pnpm/exec/lifecycle/src/runLifecycleHook.ts:119-124`.
///
/// `parent_env` is taken by value so the production caller can pass
/// `env::vars().collect()` and tests can pass a controlled fixture
/// without racing on the global process env.
pub fn build_env(
    opts: &EnvOptions<'_>,
    manifest: &Value,
    parent_env: HashMap<String, String>,
) -> EnvBuild {
    // 1. Start from the parent env, stripping every `npm_*` key
    //    plus the per-call stamps we re-derive (`NODE`, `TMPDIR`,
    //    `INIT_CWD`, `PNPM_SCRIPT_SRC_DIR`). Mirrors the
    //    `!i.match(/^npm_/)` filter at index.js:359 — `pnpm_*` keys
    //    such as `PNPM_HOME` are intentionally NOT in the filter
    //    (upstream doesn't strip them either).
    let mut env = filter_parent_env(parent_env);

    // 2. `npm_package_*` recursive stamp. Top-level keeps only
    //    name/version/config/engines/bin; recursion below those
    //    keeps everything. Mirrors index.js:377-411.
    stamp_package(&mut env, "npm_package_", manifest);

    // 3. Per-call stamping from `lifecycle()` body (index.js:74-87).
    env.insert("npm_lifecycle_event".into(), opts.stage.to_string());

    let parent_path = path_value(&env);
    let node_execpath = opts
        .node_execpath
        .map(Path::to_path_buf)
        .or_else(|| find_node_in_path(parent_path.as_deref()));
    if let Some(node) = node_execpath {
        let node_str = node.to_string_lossy().into_owned();
        env.insert("npm_node_execpath".into(), node_str.clone());
        env.insert("NODE".into(), node_str);
    }

    env.insert(
        "npm_package_json".into(),
        opts.pkg_root.join("package.json").to_string_lossy().into_owned(),
    );

    let npm_execpath = opts.npm_execpath.map(Path::to_path_buf).or_else(|| env::current_exe().ok());
    if let Some(p) = npm_execpath {
        env.insert("npm_execpath".into(), p.to_string_lossy().into_owned());
    }

    env.insert("INIT_CWD".into(), opts.init_cwd.to_string_lossy().into_owned());
    env.insert("PNPM_SCRIPT_SRC_DIR".into(), opts.script_src_dir.to_string_lossy().into_owned());

    if let Some(p) = opts.node_gyp_path {
        env.insert("npm_config_node_gyp".into(), p.to_string_lossy().into_owned());
    }
    if let Some(ua) = opts.user_agent {
        env.insert("npm_config_user_agent".into(), ua.to_string());
    }

    // 4. `extraEnv` is applied last among the makeEnv-area writes
    //    (index.js:88-92), so it overrides anything stamped above.
    for (k, v) in opts.extra_env {
        env.insert(k.clone(), v.clone());
    }

    // 5. TMPDIR under <wd>/node_modules/.tmp when !unsafe_perm.
    //    Mirrors index.js:94-104. The caller creates the dir; we
    //    only record the path and pass it back.
    let tmpdir = if opts.unsafe_perm {
        None
    } else {
        let dir = opts.pkg_root.join("node_modules").join(".tmp");
        env.insert("TMPDIR".into(), dir.to_string_lossy().into_owned());
        Some(dir)
    };

    // 6. `npm_lifecycle_script` is set in `lifecycle_` after the
    //    extraEnv overwrite (index.js:125), so the caller can never
    //    clobber it.
    env.insert("npm_lifecycle_script".into(), opts.script.to_string());

    EnvBuild { env, tmpdir }
}

/// Keep PATH (handled by the caller) and everything that does not
/// start with `npm_`; drop NODE / TMPDIR / `INIT_CWD` /
/// `PNPM_SCRIPT_SRC_DIR` because we re-derive them.
///
/// On Windows the comparison is case-insensitive because Rust's
/// `Command::env` treats env keys case-insensitively on that
/// platform (see [`Command::env`] docs). Leaving e.g. `NPM_CONFIG_FOO`
/// alongside our `npm_*` inserts would collapse at spawn time with
/// an unpredictable winner.
///
/// [`Command::env`]: https://doc.rust-lang.org/std/process/struct.Command.html#method.env
fn filter_parent_env(env: HashMap<String, String>) -> HashMap<String, String> {
    env.into_iter().filter(|(k, _)| !is_stamping_key(k, cfg!(windows))).collect()
}

/// Mirrors `!i.match(/^npm_/)` at index.js:359 plus the per-call
/// stamps we always re-derive (`NODE`, `TMPDIR`, `INIT_CWD`,
/// `PNPM_SCRIPT_SRC_DIR`). Only the `npm_*` prefix is stripped —
/// `pnpm_*` keys (e.g. `PNPM_HOME`, feature flags) are upstream-
/// preserved and pacquet does the same.
///
/// `is_windows` toggles case-insensitive matching so test code can
/// drive both branches without `#[cfg(windows)]` gating the test
/// bodies. Production callers pass `cfg!(windows)`.
fn is_stamping_key(key: &str, is_windows: bool) -> bool {
    if is_windows {
        // Byte-level prefix check: `key[..4]` would panic if byte 4
        // lands inside a multi-byte UTF-8 codepoint (e.g. a key
        // containing `𐀀` whose first byte sits at index 0). Since
        // the prefix we want is pure ASCII, comparing bytes
        // sidesteps the UTF-8 boundary question entirely.
        if key.as_bytes().get(..4).is_some_and(|b| b.eq_ignore_ascii_case(b"npm_")) {
            return true;
        }
        return ["NODE", "TMPDIR", "INIT_CWD", "PNPM_SCRIPT_SRC_DIR"]
            .iter()
            .any(|name| key.eq_ignore_ascii_case(name));
    }
    if key.starts_with("npm_") {
        return true;
    }
    matches!(key, "NODE" | "TMPDIR" | "INIT_CWD" | "PNPM_SCRIPT_SRC_DIR")
}

/// Look up the `PATH` value from `env` case-insensitively. On
/// Windows the system variable is typically `Path`, not `PATH`;
/// returning the value here lets the rest of `build_env` stay
/// independent of casing.
pub(crate) fn path_value(env: &HashMap<String, String>) -> Option<String> {
    env.iter().find_map(|(k, v)| k.eq_ignore_ascii_case("PATH").then(|| v.clone()))
}

/// Look up `node` along the supplied `PATH`. Driven by the filtered
/// `parent_env`'s PATH (not the process-global env) so `build_env`
/// stays deterministic given its inputs — matching the docstring
/// contract.
fn find_node_in_path(path: Option<&str>) -> Option<PathBuf> {
    let path = path?;
    let node_name = if cfg!(windows) { "node.exe" } else { "node" };
    env::split_paths(path).find_map(|dir| {
        let candidate = dir.join(node_name);
        candidate.is_file().then_some(candidate)
    })
}

/// `data[i]` recursion from `makeEnv`. JS arrays iterate as indexed
/// keys; objects iterate as named keys. The top-level call uses
/// prefix `npm_package_`; recursion appends `<sanitized-key>_`.
///
/// Filter at index.js:380-385: at the top level only
/// name/version/config/engines/bin are kept; once recursed under
/// one of those, everything is kept.
fn stamp_package(env: &mut HashMap<String, String>, prefix: &str, value: &Value) {
    let pairs: Vec<(String, &Value)> = match value {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Value::Array(arr) => arr.iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
        _ => return,
    };

    for (key, v) in pairs {
        if key.starts_with('_') {
            continue;
        }

        let is_top_level_keep =
            matches!(key.as_str(), "name" | "version" | "config" | "engines" | "bin");
        let in_descent = prefix.starts_with("npm_package_config_")
            || prefix.starts_with("npm_package_engines_")
            || prefix.starts_with("npm_package_bin_");
        if !is_top_level_keep && !in_descent {
            continue;
        }

        let env_key = sanitize_env_key(&format!("{prefix}{key}"));
        match v {
            Value::Object(_) | Value::Array(_) => {
                let child_prefix = format!("{env_key}_");
                stamp_package(env, &child_prefix, v);
            }
            Value::String(s) => {
                env.insert(env_key, escape_newlines(s));
            }
            Value::Number(n) => {
                env.insert(env_key, n.to_string());
            }
            Value::Bool(b) => {
                env.insert(env_key, b.to_string());
            }
            Value::Null => {
                env.insert(env_key, String::new());
            }
        }
    }
}

/// `(prefix + i).replace(/[^a-zA-Z0-9_]/g, '_')` from index.js:379.
fn sanitize_env_key(raw: &str) -> String {
    raw.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '_' { ch } else { '_' }).collect()
}

/// `env[envKey].includes('\n') ? JSON.stringify(env[envKey]) : env[envKey]`
/// from index.js:406-408. JSON-encode multi-line strings so child
/// shells don't break on embedded newlines.
fn escape_newlines(text: &str) -> String {
    if text.contains('\n') { Value::String(text.to_string()).to_string() } else { text.to_string() }
}

#[cfg(test)]
mod tests;
