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
            bundled_node_gyp_bin_in(exe.parent()?)
        })
        .as_deref()
}

/// The path arithmetic behind [`bundled_node_gyp_bin`], split out so it
/// can be exercised against a fixture directory instead of wherever the
/// test binary happens to live.
fn bundled_node_gyp_bin_in(exe_dir: &Path) -> Option<PathBuf> {
    let bin_dir = exe_dir.join(DIST_DIR).join(NODE_GYP_BIN_DIR);
    bin_dir.join(NODE_GYP_WRAPPER).is_file().then_some(bin_dir)
}

#[cfg(test)]
mod tests;
