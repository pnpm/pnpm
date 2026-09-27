use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// Directory name, relative to the executable, holding the published
/// payload that ships beside the native binary.
const DIST_DIR: &str = "dist";

/// Wrapper directory prepended to a lifecycle script's `PATH`.
const NODE_GYP_BIN_DIR: &str = "node-gyp-bin";

/// The wrapper whose presence proves the payload was shipped. Probed
/// per platform because that is what `PATH` resolution will look for:
/// finding the POSIX script says nothing about whether the `.cmd` twin
/// a Windows script needs was shipped alongside it.
const NODE_GYP_WRAPPER: &str = if cfg!(windows) { "node-gyp.cmd" } else { "node-gyp" };

/// Locate the `node-gyp` wrapper directory shipped beside the running
/// executable, for prepending to a lifecycle script's `PATH`.
///
/// pnpm ships node-gyp so that packages whose install scripts shell out
/// to it — directly, or through the `node-gyp rebuild` fallback
/// synthesized for a package with a `binding.gyp` and no install script —
/// build without the user having to install node-gyp themselves. The
/// whole dependency tree is resolved from this repo's lockfile at
/// release time, so it is frozen and reviewed per pnpm release rather
/// than resolved on the user's machine at install time.
///
/// When pnpm is launched through a symlink such as `node_modules/.bin/pnpm`,
/// the payload is also looked for beside the symlink's target.
///
/// Returns `None` when the payload is absent, which is the normal case
/// for a `cargo build` in a checkout. Lifecycle scripts then fall back to
/// whatever `node-gyp` the environment already provides.
///
/// The wrapper itself honors `npm_config_node_gyp` and only falls back to
/// the shipped copy when that is unset, so a user-supplied node-gyp keeps
/// winning without pnpm having to resolve it.
pub fn bundled_node_gyp_bin() -> Option<&'static Path> {
    static RESOLVED: OnceLock<Option<PathBuf>> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let exe = std::env::current_exe().ok()?;
            bundled_node_gyp_bin_beside(&exe)
        })
        .as_deref()
}

/// The bundled `node-gyp` entry point script, the path written into
/// `npm_config_node_gyp` when the environment does not carry a value of
/// its own. This is the default the TypeScript CLI supplies through
/// `require.resolve('node-gyp/bin/node-gyp')`; when the payload is
/// absent, as in a `cargo build` in a checkout, the variable stays
/// unset, which is also what the TypeScript CLI does when that
/// resolution fails.
pub fn bundled_node_gyp_entry() -> Option<&'static Path> {
    static RESOLVED: OnceLock<Option<PathBuf>> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let exe = std::env::current_exe().ok()?;
            bundled_node_gyp_entry_beside(&exe)
        })
        .as_deref()
}

fn bundled_node_gyp_bin_beside(exe: &Path) -> Option<PathBuf> {
    payload_beside(exe, bundled_node_gyp_bin_in)
}

fn bundled_node_gyp_entry_beside(exe: &Path) -> Option<PathBuf> {
    payload_beside(exe, bundled_node_gyp_entry_in)
}

/// Probe the payload beside `exe`, through the unresolved path first.
///
/// `current_exe` is the path pnpm was launched through on some platforms,
/// macOS among them. The unresolved path is tried first because
/// canonicalizing a path on a Windows network drive yields a verbatim
/// `\\?\UNC` path, which not every program that searches `PATH` accepts.
fn payload_beside(exe: &Path, locate_in: fn(&Path) -> Option<PathBuf>) -> Option<PathBuf> {
    locate_in(exe.parent()?)
        .or_else(|| {
            let exe = dunce::canonicalize(exe).ok()?;
            locate_in(exe.parent()?)
        })
}

/// The path arithmetic behind [`bundled_node_gyp_bin`], split out so it
/// can be exercised against a fixture directory instead of wherever the
/// test binary happens to live.
fn bundled_node_gyp_bin_in(exe_dir: &Path) -> Option<PathBuf> {
    let bin_dir = exe_dir.join(DIST_DIR).join(NODE_GYP_BIN_DIR);
    bin_dir
        .join(NODE_GYP_WRAPPER)
        .is_file()
        .then_some(bin_dir)
}

/// The path arithmetic behind [`bundled_node_gyp_entry`], shaped like
/// [`bundled_node_gyp_bin_in`] so fixtures can exercise it too. The
/// entry is the script `dist/node-gyp-bin/node-gyp` runs as its
/// fallback, so the variable and the wrapper name the same file.
fn bundled_node_gyp_entry_in(exe_dir: &Path) -> Option<PathBuf> {
    let entry = exe_dir
        .join(DIST_DIR)
        .join("node_modules")
        .join("node-gyp")
        .join("bin")
        .join("node-gyp.js");
    entry.is_file().then_some(entry)
}

#[cfg(test)]
mod tests;
