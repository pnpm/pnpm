use super::{ScriptsPrependNodePath, extend_path};
use pretty_assertions::assert_eq;
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

#[cfg(unix)]
const SEP: char = ':';
#[cfg(windows)]
const SEP: char = ';';

fn segments(path: &OsString) -> Vec<String> {
    env::split_paths(path).map(|path| path.to_string_lossy().into_owned()).collect()
}

/// Ports `test('the path to node-gyp should be added after the path
/// to node_modules/.bin')` from
/// <https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/test/extendPath.test.js#L5-L8>.
///
/// The only upstream test for extendPath; assertion is purely
/// ordering between `node_modules/.bin` and the bundled `node-gyp`.
#[test]
fn node_gyp_comes_after_node_modules_dot_bin() {
    let wd = Path::new("/Users/x/project");
    let node_gyp = PathBuf::from("/lib/node-gyp-bin");
    let extra: Vec<PathBuf> = vec![];
    let path = extend_path(wd, None, Some(&node_gyp), &extra, ScriptsPrependNodePath::Never, None);
    let parts = segments(&path);
    let bin_idx = parts
        .iter()
        .position(|p| {
            p.ends_with(&format!(
                "project{}node_modules{}.bin",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR,
            ))
        })
        .unwrap_or_else(|| panic!("missing node_modules/.bin in {parts:?}"));
    let gyp_idx = parts
        .iter()
        .position(|p| p.contains("node-gyp-bin"))
        .unwrap_or_else(|| panic!("missing node-gyp-bin in {parts:?}"));
    assert!(
        bin_idx < gyp_idx,
        ".bin must precede node-gyp; got bin@{bin_idx}, gyp@{gyp_idx} in {parts:?}",
    );
}

/// `wd` with no `node_modules` segment yields exactly one
/// `<wd>/node_modules/.bin` entry — no ancestor walk happens. Mirrors
/// the upstream split where `wd.split('/node_modules/')` returns a
/// single-element array and the loop body never runs.
#[test]
fn no_ancestors_when_wd_has_no_node_modules_segment() {
    let wd = Path::new("/home/me/project");
    let extra: Vec<PathBuf> = vec![];
    let path = extend_path(wd, None, None, &extra, ScriptsPrependNodePath::Never, None);
    let parts = segments(&path);
    assert_eq!(parts.len(), 1, "expected exactly one .bin entry, got {parts:?}");
    assert!(parts[0].ends_with(".bin"), "must be a .bin path: {:?}", parts[0]);
}

/// Two-level pnpm virtual store wd:
/// `<root>/node_modules/.pnpm/foo@1.0.0/node_modules/foo`. The walk
/// must produce three `.bin` paths, ordered deepest-first.
///
/// Unix-only because `path::absolute("/proj")` on Windows resolves
/// against the current drive (`C:\proj`), which makes the hard-coded
/// expected values racy. The structural invariants (count + deepest-
/// first ordering) are covered platform-neutrally in
/// [`virtual_store_walk_orders_deepest_first`] below.
#[cfg(unix)]
#[test]
fn pnpm_virtual_store_layout_yields_three_bins_deepest_first() {
    let wd = Path::new("/proj/node_modules/.pnpm/foo@1.0.0/node_modules/foo");
    let extra: Vec<PathBuf> = vec![];
    let path = extend_path(wd, None, None, &extra, ScriptsPrependNodePath::Never, None);
    let parts = segments(&path);
    assert_eq!(
        parts,
        vec![
            "/proj/node_modules/.pnpm/foo@1.0.0/node_modules/foo/node_modules/.bin".to_string(),
            "/proj/node_modules/.pnpm/foo@1.0.0/node_modules/.bin".to_string(),
            "/proj/node_modules/.bin".to_string(),
        ],
    );
}

/// Platform-neutral version of the virtual-store walk test: assert
/// the count and the deepest-first ordering (each entry must be a
/// strict prefix of the entry before it after stripping the trailing
/// `node_modules/.bin`) without anchoring to any absolute root.
#[test]
fn virtual_store_walk_orders_deepest_first() {
    let wd = Path::new("proj")
        .join("node_modules")
        .join(".pnpm")
        .join("foo@1.0.0")
        .join("node_modules")
        .join("foo");
    let extra: Vec<PathBuf> = vec![];
    let path = extend_path(&wd, None, None, &extra, ScriptsPrependNodePath::Never, None);
    let parts = segments(&path);
    assert_eq!(parts.len(), 3, "expected three bin paths, got {parts:?}");
    for window in parts.windows(2) {
        let deeper = &window[0];
        let shallower = &window[1];
        assert!(deeper.len() > shallower.len(), "{deeper:?} must be deeper than {shallower:?}");
        assert!(deeper.ends_with(".bin") && shallower.ends_with(".bin"));
    }
}

/// `extra_bin_paths` slot in after the .bin walk and after node-gyp.
/// Upstream order at lib/extendPath.js:6-19:
///   `pathArr = [...extraBinPaths]` then unshift node-gyp, then
///   unshift each .bin → final order [bins..., nodeGyp, ...extraBinPaths].
#[test]
fn extra_bin_paths_come_after_bins_and_node_gyp() {
    let wd = Path::new("/proj");
    let node_gyp = PathBuf::from("/bundled/node-gyp-bin");
    let extra: Vec<PathBuf> = vec![PathBuf::from("/extra/one"), PathBuf::from("/extra/two")];
    let path = extend_path(wd, None, Some(&node_gyp), &extra, ScriptsPrependNodePath::Never, None);
    let parts = segments(&path);
    let bin_idx =
        parts.iter().position(|part| part.contains("proj") && part.ends_with(".bin")).unwrap();
    let gyp_idx = parts.iter().position(|part| part.contains("node-gyp-bin")).unwrap();
    let extra1_idx =
        parts.iter().position(|part| part == "/extra/one" || part == r"\extra\one").unwrap();
    let extra2_idx =
        parts.iter().position(|part| part == "/extra/two" || part == r"\extra\two").unwrap();
    assert!(
        bin_idx < gyp_idx && gyp_idx < extra1_idx && extra1_idx < extra2_idx,
        "expected order .bin < nodeGyp < extra1 < extra2; got {parts:?}",
    );
}

/// `original_path` is appended verbatim at the end of the joined
/// PATH. The system PATH thus stays lowest-priority while the
/// .bin walk and node-gyp take precedence.
#[test]
fn original_path_is_appended_last() {
    let wd = Path::new("/proj");
    let extra: Vec<PathBuf> = vec![];
    let sys_path = {
        let mut text = OsString::new();
        text.push("/usr/local/bin");
        text.push(SEP.to_string());
        text.push("/usr/bin");
        text
    };
    let path = extend_path(wd, Some(&sys_path), None, &extra, ScriptsPrependNodePath::Never, None);
    let parts = segments(&path);
    assert_eq!(parts.len(), 3, "1 bin + 2 sys = 3 entries, got {parts:?}");
    assert_eq!(parts[1], "/usr/local/bin");
    assert_eq!(parts[2], "/usr/bin");
}

/// `scripts_prepend_node_path: Always` appends `dirname(node)` after
/// `extra_bin_paths` but before originalPath.
#[test]
fn scripts_prepend_node_path_always_appends_dirname_of_node() {
    let wd = Path::new("/proj");
    let node = PathBuf::from("/opt/node/bin/node");
    let extra: Vec<PathBuf> = vec![];
    let path = extend_path(wd, None, None, &extra, ScriptsPrependNodePath::Always, Some(&node));
    let parts = segments(&path);
    assert!(
        parts.iter().any(|part| part == "/opt/node/bin"),
        "expected dirname(node) in PATH, got {parts:?}",
    );
}

/// Regression: a path component containing the platform separator
/// must not cause `extend_path` to drop the computed entries.
/// Upstream's `pathArr.join(':')` produces the embedded-separator
/// string verbatim; pacquet must match. Skipping on Windows where
/// `;` is far less likely to appear in real paths, but the
/// invariant holds there too.
#[cfg(unix)]
#[test]
fn separator_in_path_component_does_not_drop_other_entries() {
    // A bin path that itself contains a colon — exotic, but valid
    // on POSIX. `env::join_paths` would reject it; the naive join
    // mirrors upstream by embedding it verbatim.
    let wd = Path::new("/proj");
    let weird = PathBuf::from("/tmp/a:b/.bin");
    let path = extend_path(
        wd,
        None,
        None,
        std::slice::from_ref(&weird),
        ScriptsPrependNodePath::Never,
        None,
    );
    let text = path.to_string_lossy();
    assert!(text.contains("/proj/node_modules/.bin"), "wd .bin must survive: {text:?}");
    assert!(text.contains("/tmp/a:b/.bin"), "the weird extra path must survive verbatim: {text:?}");
}

/// Tri-state: `Never` and `WarnOnly` both skip the actual prepend
/// (mirroring the `cfgsetting === true` gate at lib/extendPath.js:32).
/// `WarnOnly` would emit a warning upstream; pacquet's reporter-side
/// emission is decoupled from extendPath, so this function just
/// skips.
#[test]
fn scripts_prepend_node_path_never_and_warn_only_do_not_prepend() {
    let wd = Path::new("/proj");
    let node = PathBuf::from("/opt/node/bin/node");
    let extra: Vec<PathBuf> = vec![];
    for variant in [ScriptsPrependNodePath::Never, ScriptsPrependNodePath::WarnOnly] {
        let path = extend_path(wd, None, None, &extra, variant, Some(&node));
        let parts = segments(&path);
        assert!(
            !parts.iter().any(|part| part == "/opt/node/bin"),
            "variant {variant:?} must not prepend dirname(node), got {parts:?}",
        );
    }
}
