use std::{
    env,
    ffi::{OsStr, OsString},
    path::{self, Path, PathBuf},
};

/// Controls whether the dir containing the current `node` interpreter
/// is appended to PATH. Tri-state from
/// <https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/lib/extendPath.js#L29-L61>.
///
/// `pacquet-config` mirrors this enum with its own yaml-deserializable
/// type (upstream's `scriptsPrependNodePath: boolean | 'warn-only'`
/// shape) and converts to this one at the call site, so the executor
/// crate stays free of serde and Config wiring.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScriptsPrependNodePath {
    /// `scriptsPrependNodePath: true` — always prepend.
    Always,
    /// `scriptsPrependNodePath: false` (or `null`) — never prepend.
    #[default]
    Never,
    /// `scriptsPrependNodePath: 'warn-only'` — emit a warning if the
    /// node in PATH differs from `process.execPath`, but do not
    /// prepend.
    WarnOnly,
}

/// Build the `PATH` env value for a lifecycle script spawn.
///
/// Ports `extendPath` from
/// <https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/lib/extendPath.js#L5-L27>.
///
/// Order, highest-priority first:
/// 1. The wd's own `<wd>/node_modules/.bin`,
/// 2. Each ancestor `node_modules/.bin` walking back up through the
///    `node_modules/` segments of `wd`,
/// 3. The bundled `node-gyp-bin` directory (when supplied),
/// 4. `extra_bin_paths` (caller-supplied),
/// 5. `dirname(node_execpath)` when `scripts_prepend_node_path` is
///    [`Always`](ScriptsPrependNodePath::Always),
/// 6. `original_path` (typically the inherited system PATH).
#[must_use]
pub fn extend_path(
    wd: &Path,
    original_path: Option<&OsString>,
    node_gyp_bin: Option<&Path>,
    extra_bin_paths: &[PathBuf],
    scripts_prepend_node_path: ScriptsPrependNodePath,
    node_execpath: Option<&Path>,
) -> OsString {
    let mut path_arr: Vec<PathBuf> = Vec::new();

    // 1+2. Walk the wd's node_modules ancestors, deepest first.
    for bin in ancestor_node_modules_bins(wd) {
        path_arr.push(bin);
    }

    // 3. Bundled node-gyp-bin.
    if let Some(p) = node_gyp_bin {
        path_arr.push(p.to_path_buf());
    }

    // 4. Caller-supplied extra paths.
    path_arr.extend_from_slice(extra_bin_paths);

    // 5. dirname(node) when scriptsPrependNodePath is `Always`.
    //    `WarnOnly` only emits a warning upstream; the actual prepend
    //    is gated on `cfgsetting === true` at lib/extendPath.js:32.
    //    We omit the warn-emission here; the caller (with reporter
    //    context) is a better place for it.
    if scripts_prepend_node_path == ScriptsPrependNodePath::Always
        && let Some(node) = node_execpath
        && let Some(parent) = node.parent()
    {
        path_arr.push(parent.to_path_buf());
    }

    // 6. originalPath at the end.
    let mut joined: Vec<PathBuf> = path_arr;
    if let Some(orig) = original_path {
        for p in env::split_paths(orig) {
            joined.push(p);
        }
    }

    join_paths_lossy(&joined)
}

/// Join `paths` with the platform PATH separator (`;` on Windows,
/// `:` elsewhere). Mirrors upstream's
/// `pathArr.join(process.platform === 'win32' ? ';' : ':')` at
/// <https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/lib/extendPath.js#L26>
/// exactly — including the lack of validation. If a path component
/// itself contains the separator the spawned shell sees an embedded
/// entry, which is the same behavior the upstream string-join
/// produces. `std::env::join_paths` would have erred and dropped
/// the entire computed PATH in that case.
fn join_paths_lossy(paths: &[PathBuf]) -> OsString {
    let sep: &OsStr = if cfg!(windows) { OsStr::new(";") } else { OsStr::new(":") };
    let mut out = OsString::new();
    for (i, p) in paths.iter().enumerate() {
        if i > 0 {
            out.push(sep);
        }
        out.push(p);
    }
    out
}

/// Returns the sequence of `node_modules/.bin` directories implied by
/// `wd`, ordered deepest-first to match the upstream `unshift` walk
/// at lib/extendPath.js:14-18.
///
/// The walk mirrors upstream's `wd.split(/[\\/]node_modules[\\/]/)`
/// scheme: it does *not* walk parent directories beyond the first
/// `node_modules/` ancestor of `wd`. If `wd` contains no
/// `node_modules/` segment, only `<wd>/node_modules/.bin` is
/// produced.
fn ancestor_node_modules_bins(wd: &Path) -> Vec<PathBuf> {
    let normalized = normalize_for_split(wd);
    let parts: Vec<&str> = normalized.split("/node_modules/").collect();

    // First part is the project root (everything before the first
    // `/node_modules/` segment); remaining parts are intermediate
    // `node_modules/<pp>` slots.
    let (head, tail) = parts.split_first().expect("split always yields at least one element");

    // Match upstream's `path.resolve(p.shift())` at
    // lib/extendPath.js:8: absolute paths stay as-is, relative
    // paths anchor against the process cwd, and an empty head
    // (which happens when `wd` starts with `node_modules/`) means
    // "use cwd". This anchoring carries upstream's cwd-dependence
    // when the caller passes a relative wd — pacquet's production
    // call sites always pass an absolute pkg_root, so the
    // dependency is a non-issue in practice.
    let mut acc = if head.is_empty() {
        env::current_dir().unwrap_or_else(|_| PathBuf::new())
    } else {
        let head_path = PathBuf::from(head);
        path::absolute(&head_path).unwrap_or(head_path)
    };

    let mut bins: Vec<PathBuf> = Vec::with_capacity(parts.len());

    // Each pp in the tail contributes a `${acc}/node_modules/.bin`
    // (from the parent slot), then `acc` advances to
    // `${acc}/node_modules/${pp}`. After the loop, the final
    // `${acc}/node_modules/.bin` is the deepest one (the wd itself).
    for pp in tail {
        bins.push(acc.join("node_modules").join(".bin"));
        acc = acc.join("node_modules").join(pp);
    }
    bins.push(acc.join("node_modules").join(".bin"));

    // Upstream `unshift`es each entry so the deepest .bin ends up
    // first; collect in the natural order then reverse.
    bins.reverse();
    bins
}

fn normalize_for_split(wd: &Path) -> String {
    let text = wd.to_string_lossy().into_owned();
    if cfg!(windows) { text.replace('\\', "/") } else { text }
}

#[cfg(test)]
mod tests;
