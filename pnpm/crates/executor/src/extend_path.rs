use std::{
    collections::HashSet,
    env,
    ffi::{OsStr, OsString},
    path::{self, Path, PathBuf},
};

/// Controls whether the dir containing the current `node` interpreter
/// is appended to PATH. Tri-state, corresponding to the
/// `scriptsPrependNodePath: boolean | 'warn-only'` config setting.
///
/// `pnpm-config` mirrors this enum with its own yaml-deserializable
/// type and converts to this one at the call site, so the executor
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
/// Order, highest-priority first:
/// 1. The wd's own bin directory: `wd_bin_dir` when the caller knows
///    where `modulesDir` put it, and `<wd>/node_modules/.bin` otherwise,
/// 2. Each ancestor `node_modules/.bin` walking back up through the
///    `node_modules/` segments of `wd`. These are dependency slots, whose
///    own dependencies are installed under `node_modules` whatever
///    `modulesDir` says, so `wd_bin_dir` does not apply to them,
/// 3. The bundled `node-gyp-bin` directory (when supplied),
/// 4. `extra_bin_paths` (caller-supplied),
/// 5. `dirname(node_execpath)` when `scripts_prepend_node_path` is
///    [`Always`](ScriptsPrependNodePath::Always),
/// 6. `original_path` (typically the inherited system PATH), minus the
///    entries already listed above.
#[must_use]
pub fn extend_path(
    wd: &Path,
    wd_bin_dir: Option<&Path>,
    original_path: Option<&OsString>,
    node_gyp_bin: Option<&Path>,
    extra_bin_paths: &[PathBuf],
    scripts_prepend_node_path: ScriptsPrependNodePath,
    node_execpath: Option<&Path>,
) -> OsString {
    let mut path_arr: Vec<PathBuf> = Vec::new();

    // 1+2. Walk the wd's node_modules ancestors, deepest first. The first
    // entry is the wd's own, which `wd_bin_dir` overrides.
    let mut ancestors = ancestor_node_modules_bins(wd);
    if let Some(bin) = wd_bin_dir
        && let Some(own) = ancestors.first_mut()
    {
        *own = bin.to_path_buf();
    }
    path_arr.extend(ancestors);

    // 3. Bundled node-gyp-bin.
    if let Some(p) = node_gyp_bin {
        path_arr.push(p.to_path_buf());
    }

    // 4. Caller-supplied extra paths.
    path_arr.extend_from_slice(extra_bin_paths);

    // 5. dirname(node) when scriptsPrependNodePath is `Always`.
    //    `WarnOnly` only emits a warning; the actual prepend is gated
    //    on the setting being `true`. We omit the warn-emission here;
    //    the caller (with reporter context) is a better place for it.
    if scripts_prepend_node_path == ScriptsPrependNodePath::Always
        && let Some(node) = node_execpath
        && let Some(parent) = node.parent()
    {
        path_arr.push(parent.to_path_buf());
    }

    // 6. originalPath at the end.
    if let Some(orig) = original_path {
        let added: HashSet<OsString> = path_arr
            .iter()
            .map(|entry| entry.as_os_str().to_os_string())
            .collect();
        path_arr.extend(env::split_paths(orig).filter(|entry| !added.contains(entry.as_os_str())));
    }

    join_paths_lossy(&path_arr)
}

/// Join `paths` with the platform PATH separator (`;` on Windows,
/// `:` elsewhere) by plain string concatenation — no validation. If a
/// path component itself contains the separator the spawned shell sees
/// an embedded entry; `std::env::join_paths` would instead have erred
/// and dropped the entire computed PATH in that case.
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
/// `wd`, ordered deepest-first.
///
/// The walk splits `wd` on its `node_modules/` segments: it does *not*
/// walk parent directories beyond the first `node_modules/` ancestor of
/// `wd`.
fn ancestor_node_modules_bins(wd: &Path) -> Vec<PathBuf> {
    let normalized = normalize_for_split(wd);
    let parts: Vec<&str> = normalized.split("/node_modules/").collect();

    // First part is the project root (everything before the first
    // `/node_modules/` segment); remaining parts are intermediate
    // `node_modules/<pp>` slots.
    let (head, tail) = parts.split_first().expect("split always yields at least one element");

    // Resolve the head like `path.resolve`: absolute paths stay as-is,
    // relative paths anchor against the process cwd, and an empty head
    // (which happens when `wd` starts with `node_modules/`) means
    // "use cwd". This anchoring makes the result cwd-dependent when the
    // caller passes a relative wd — pacquet's production call sites
    // always pass an absolute pkg_root, so the dependency is a
    // non-issue in practice.
    let mut acc = if head.is_empty() {
        env::current_dir().unwrap_or_else(|_| PathBuf::new())
    } else {
        let head_path = if cfg!(windows) {
            PathBuf::from(head.replace('/', r"\"))
        } else {
            PathBuf::from(head)
        };
        path::absolute(&head_path).unwrap_or(head_path)
    };

    let mut bins: Vec<PathBuf> = Vec::with_capacity(parts.len());

    // Each pp in the tail contributes a `${acc}/node_modules/.bin`
    // (from the parent slot), then `acc` advances to
    // `${acc}/node_modules/${pp}`. After the loop, the final
    // `${acc}/node_modules/.bin` is the deepest one (the wd itself).
    for pp in tail {
        bins.push(acc.join("node_modules").join(".bin"));
        acc.push("node_modules");
        pnpm_fs::push_slash_separated_path(&mut acc, pp);
    }
    bins.push(acc.join("node_modules").join(".bin"));

    // The deepest .bin must end up first; collect in the natural order
    // then reverse.
    bins.reverse();
    bins
}

fn normalize_for_split(wd: &Path) -> String {
    let text = wd.to_string_lossy().into_owned();
    if cfg!(windows) { text.replace('\\', "/") } else { text }
}

#[cfg(test)]
mod tests;
