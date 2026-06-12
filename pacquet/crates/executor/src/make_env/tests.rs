use super::{
    EnvOptions, build_env, escape_newlines, is_stamping_key, sanitize_env_key, stamp_package,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{collections::HashMap, path::Path};

fn empty_extra() -> HashMap<String, String> {
    HashMap::new()
}

fn base_opts<'a>(
    pkg_root: &'a Path,
    init_cwd: &'a Path,
    extra_env: &'a HashMap<String, String>,
) -> EnvOptions<'a> {
    EnvOptions {
        stage: "postinstall",
        script: "echo hi",
        pkg_root,
        init_cwd,
        script_src_dir: pkg_root,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        extra_env,
    }
}

/// Ports `test('makeEnv')` from
/// <https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/test/index.js#L97-L124>.
///
/// Four invariants we mirror:
/// - top-level `npm_package_name` is set from the manifest's `name`,
/// - package-local config like `_myPackage` keys are NOT promoted to
///   `npm_package_config_*`,
/// - `npm_*` keys leaked from the parent env are stripped (upstream's
///   `!i.match(/^npm_/)` filter at `index.js:359`),
/// - everything else passes through — including `pnpm_*` keys like
///   `PNPM_HOME`, which upstream does not filter.
#[test]
fn make_env_stamps_top_level_keys_and_strips_npm_config_leakage() {
    let mut parent = HashMap::new();
    parent.insert("PATH".into(), "/usr/bin".into());
    parent.insert("npm_config_enteente".into(), "should-be-stripped".into());
    parent.insert("PNPM_HOME".into(), "/opt/pnpm".into());
    parent.insert("HOME".into(), "/home/me".into());

    let manifest = json!({
        "name": "@scope/pkg",
        "version": "1.2.3",
        "config": { "myKey": "myValue" },
        "_myPackage": { "secret": "ignored" },
        "scripts": { "postinstall": "noop" },
    });

    let pkg_root = Path::new("/tmp/pkg-x");
    let extra = empty_extra();
    let built = build_env(&base_opts(pkg_root, pkg_root, &extra), &manifest, parent);

    assert_eq!(built.env.get("npm_package_name").map(String::as_str), Some("@scope/pkg"));
    assert_eq!(built.env.get("npm_package_version").map(String::as_str), Some("1.2.3"));
    assert_eq!(built.env.get("npm_package_config_myKey").map(String::as_str), Some("myValue"));
    assert!(
        !built.env.contains_key("npm_package__myPackage_secret"),
        "underscore-prefixed manifest keys must be ignored",
    );
    assert!(
        !built.env.contains_key("npm_config_enteente"),
        "npm_config_* must be stripped from parent env: {:?}",
        built.env,
    );
    assert_eq!(
        built.env.get("PNPM_HOME").map(String::as_str),
        Some("/opt/pnpm"),
        "pnpm_* (incl. PNPM_HOME) keys are NOT in upstream's strip filter — they must pass through",
    );
    assert_eq!(
        built.env.get("HOME").map(String::as_str),
        Some("/home/me"),
        "non-npm parent keys are preserved",
    );
}

/// `scripts` is NOT in the top-level keep-list at index.js:381, so it
/// must not become `npm_package_scripts_*`. Catches the most common
/// way the filter could regress.
#[test]
fn make_env_drops_non_keep_listed_top_level_keys() {
    let manifest = json!({
        "name": "x",
        "version": "0.1.0",
        "scripts": { "postinstall": "echo hi", "test": "exit 1" },
        "dependencies": { "foo": "1.0.0" },
        "homepage": "https://example.com",
    });

    let pkg_root = Path::new("/tmp/x");
    let extra = empty_extra();
    let built = build_env(&base_opts(pkg_root, pkg_root, &extra), &manifest, HashMap::new());

    for not_kept in
        ["npm_package_scripts_postinstall", "npm_package_dependencies_foo", "npm_package_homepage"]
    {
        assert!(!built.env.contains_key(not_kept), "{not_kept} must be filtered out");
    }
}

/// `npm_lifecycle_event`, `npm_lifecycle_script`, `npm_package_json`,
/// `INIT_CWD`, and `PNPM_SCRIPT_SRC_DIR` all come from the lifecycle
/// wrapper, not from the manifest. Mirrors index.js:74-86 + the pnpm
/// wrapper at runLifecycleHook.ts:119-124.
#[test]
fn make_env_stamps_lifecycle_specific_keys() {
    let pkg_root = Path::new("/tmp/y");
    let init_cwd = Path::new("/tmp/projects/y");
    let extra = empty_extra();

    let opts = EnvOptions {
        stage: "preinstall",
        script: "node x.js",
        pkg_root,
        init_cwd,
        script_src_dir: pkg_root,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        extra_env: &extra,
    };

    let built = build_env(&opts, &json!({ "name": "y", "version": "1.0.0" }), HashMap::new());

    // Compute expected paths through the same `join` so the
    // assertions are correct on Windows (`\\` separator) as well as
    // POSIX. Path-separator handling itself is `std`'s job — these
    // tests verify build_env's mapping, not separator policy.
    let expected_package_json = pkg_root.join("package.json").to_string_lossy().into_owned();
    let expected_init_cwd = init_cwd.to_string_lossy().into_owned();
    let expected_src_dir = pkg_root.to_string_lossy().into_owned();

    assert_eq!(built.env.get("npm_lifecycle_event").map(String::as_str), Some("preinstall"));
    assert_eq!(built.env.get("npm_lifecycle_script").map(String::as_str), Some("node x.js"));
    assert_eq!(built.env.get("npm_package_json"), Some(&expected_package_json));
    assert_eq!(built.env.get("INIT_CWD"), Some(&expected_init_cwd));
    assert_eq!(built.env.get("PNPM_SCRIPT_SRC_DIR"), Some(&expected_src_dir));
}

/// `unsafe_perm: true` skips both the TMPDIR creation and the env
/// stamp; `unsafe_perm: false` records the path but does NOT create
/// the directory (that's the caller's job).
#[test]
fn make_env_tmpdir_gating_mirrors_unsafe_perm() {
    let pkg_root = Path::new("/tmp/z");
    let extra = empty_extra();

    let mut opts = base_opts(pkg_root, pkg_root, &extra);
    opts.unsafe_perm = true;
    let built = build_env(&opts, &json!({"name":"z","version":"0"}), HashMap::new());
    assert!(built.tmpdir.is_none());
    assert!(!built.env.contains_key("TMPDIR"));

    opts.unsafe_perm = false;
    let built = build_env(&opts, &json!({"name":"z","version":"0"}), HashMap::new());
    let expected_tmpdir = pkg_root.join("node_modules").join(".tmp");
    assert_eq!(built.tmpdir.as_deref(), Some(expected_tmpdir.as_path()));
    assert_eq!(built.env.get("TMPDIR"), Some(&expected_tmpdir.to_string_lossy().into_owned()));
}

/// `extra_env` is applied AFTER the lifecycle-area writes, so it can
/// override `INIT_CWD` etc. — matches index.js:88-92's `Object.entries(opts.extraEnv)`
/// loop order. But `npm_lifecycle_script` is stamped *after* extraEnv
/// (set in lifecycle_ at index.js:125), so the caller can never
/// clobber it.
#[test]
fn extra_env_overrides_writes_except_lifecycle_script() {
    let pkg_root = Path::new("/tmp/w");
    let mut extra = HashMap::new();
    extra.insert("INIT_CWD".into(), "/overridden".into());
    extra.insert("npm_lifecycle_script".into(), "FAKE".into());
    extra.insert("CUSTOM".into(), "hello".into());

    let opts = EnvOptions {
        stage: "postinstall",
        script: "REAL",
        pkg_root,
        init_cwd: Path::new("/original"),
        script_src_dir: pkg_root,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: None,
        unsafe_perm: true,
        extra_env: &extra,
    };

    let built = build_env(&opts, &json!({"name":"w","version":"0"}), HashMap::new());

    assert_eq!(built.env.get("INIT_CWD").map(String::as_str), Some("/overridden"));
    assert_eq!(built.env.get("npm_lifecycle_script").map(String::as_str), Some("REAL"));
    assert_eq!(built.env.get("CUSTOM").map(String::as_str), Some("hello"));
}

/// Recursion goes one level into `config`, `engines`, `bin` but
/// keeps everything inside them — including nested objects.
#[test]
fn stamp_package_recurses_into_kept_buckets() {
    let mut env = HashMap::new();
    stamp_package(
        &mut env,
        "npm_package_",
        &json!({
            "name": "pkg",
            "config": { "port": 3000, "deep": { "nested": "value" } },
            "engines": { "node": ">=18" },
            "bin": { "foo": "./bin/foo.js" },
        }),
    );
    assert_eq!(env.get("npm_package_name").map(String::as_str), Some("pkg"));
    assert_eq!(env.get("npm_package_config_port").map(String::as_str), Some("3000"));
    assert_eq!(
        env.get("npm_package_config_deep_nested").map(String::as_str),
        Some("value"),
        "recursion must keep going beneath config/* — only the top-level filter restricts",
    );
    assert_eq!(env.get("npm_package_engines_node").map(String::as_str), Some(">=18"));
    assert_eq!(env.get("npm_package_bin_foo").map(String::as_str), Some("./bin/foo.js"));
}

/// Array indices become numeric keys (`bin[0]`, `bin[1]`, ...) — JS
/// iterates `for (i in array)` as strings, and the recursion handles
/// it the same way as object keys.
#[test]
fn stamp_package_handles_arrays() {
    let mut env = HashMap::new();
    stamp_package(&mut env, "npm_package_", &json!({"name":"a","bin":["./a","./b"]}));
    assert_eq!(env.get("npm_package_bin_0").map(String::as_str), Some("./a"));
    assert_eq!(env.get("npm_package_bin_1").map(String::as_str), Some("./b"));
}

/// `(prefix + i).replace(/[^a-zA-Z0-9_]/g, '_')` from index.js:379.
/// Scoped names, dashes, dots all collapse to `_`.
#[test]
fn sanitize_env_key_matches_upstream_regex() {
    assert_eq!(sanitize_env_key("npm_package_name"), "npm_package_name");
    assert_eq!(sanitize_env_key("npm_package_@scope/foo"), "npm_package__scope_foo");
    assert_eq!(sanitize_env_key("npm_package_a-b.c"), "npm_package_a_b_c");
    assert_eq!(sanitize_env_key("npm_package_já"), "npm_package_j_");
}

/// On POSIX, env keys are case-sensitive: `NPM_CONFIG_FOO` is a
/// different variable than `npm_config_foo`, so only the lowercase
/// `npm_` prefix matches — matching upstream's `/^npm_/` regex
/// exactly.
#[test]
fn is_stamping_key_is_case_sensitive_on_posix() {
    assert!(is_stamping_key("npm_config_user_agent", false));
    assert!(!is_stamping_key("NPM_CONFIG_USER_AGENT", false));
    assert!(!is_stamping_key("Npm_Lifecycle_Event", false));
    assert!(is_stamping_key("NODE", false));
    assert!(!is_stamping_key("Node", false));
    assert!(!is_stamping_key("node", false));
    assert!(is_stamping_key("TMPDIR", false));
    assert!(is_stamping_key("INIT_CWD", false));
    assert!(is_stamping_key("PNPM_SCRIPT_SRC_DIR", false));
    // pnpm_* keys other than the well-known stamps survive.
    assert!(!is_stamping_key("PNPM_HOME", false));
}

/// Regression: the byte-level prefix check inside the Windows
/// branch must not panic on non-ASCII keys whose UTF-8 byte
/// representation crosses byte 4. Slicing `key[..4]` panics for
/// e.g. `"x𐀀"` (1 ASCII byte + a 4-byte codepoint), since byte 4
/// lands inside the codepoint.
#[test]
fn is_stamping_key_handles_non_ascii_keys_without_panicking() {
    // 1 ASCII byte + 4-byte UTF-8 codepoint = 5 bytes; byte 4 is
    // mid-char. Comparison must be byte-safe.
    assert!(!is_stamping_key("x\u{10000}", true));
    assert!(!is_stamping_key("x\u{10000}", false));
    // 2-byte codepoint repeated: byte 4 IS a char boundary, but
    // the leading bytes are not ASCII so the comparison must
    // still return false.
    assert!(!is_stamping_key("\u{0419}\u{0419}", true));
    // 1-character ASCII (< 4 bytes): byte slice short-circuits.
    assert!(!is_stamping_key("abc", true));
    assert!(!is_stamping_key("", true));
}

/// On Windows, Rust's `Command::env` treats env keys
/// case-insensitively, so `NPM_CONFIG_FOO` and `npm_config_foo`
/// refer to the same variable. We must strip the entire family
/// case-insensitively or our `npm_*` inserts collide at spawn time
/// with an unpredictable winner.
#[test]
fn is_stamping_key_is_case_insensitive_on_windows() {
    assert!(is_stamping_key("npm_config_user_agent", true));
    assert!(is_stamping_key("NPM_CONFIG_USER_AGENT", true));
    assert!(is_stamping_key("Npm_Lifecycle_Event", true));
    assert!(is_stamping_key("NODE", true));
    assert!(is_stamping_key("Node", true));
    assert!(is_stamping_key("node", true));
    assert!(is_stamping_key("tmpdir", true));
    assert!(is_stamping_key("init_cwd", true));
    assert!(is_stamping_key("pnpm_script_src_dir", true));
    // Edge: short keys shouldn't accidentally match `npm_`.
    assert!(!is_stamping_key("NPM", true));
    assert!(!is_stamping_key("npm", true));
    // pnpm_* keys other than the well-known stamps still survive.
    assert!(!is_stamping_key("PNPM_HOME", true));
    assert!(!is_stamping_key("pnpm_home", true));
}

/// Multi-line strings get JSON-encoded so child shells don't see a
/// literal newline. Mirrors index.js:406-408.
#[test]
fn escape_newlines_json_encodes_multi_line_only() {
    assert_eq!(escape_newlines("plain"), "plain");
    assert_eq!(escape_newlines("a\nb"), r#""a\nb""#);
    // Single-line strings with quotes/backslashes pass through verbatim
    // — matches the JS `s.includes('\n') ? JSON.stringify(s) : s` exactly.
    assert_eq!(escape_newlines(r#"has "quotes""#), r#"has "quotes""#);
}
