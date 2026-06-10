use super::{filesystem_root, host_can_link_between_dirs, next_path, resolve_store_dir};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

// The `PrefixProbe` machinery below is only used by tests gated on
// `cfg(unix)` (the cross-volume scenarios construct absolute Unix paths
// like `/Volumes/src/...`). On Windows, every consumer is excluded, so
// gate the fake itself to keep clippy's `dead_code` lint happy under
// `-D warnings`.
#[cfg(unix)]
use crate::api::LinkProbe;
#[cfg(unix)]
use std::sync::Mutex;

/// `next_path` walks one path segment at a time from `from` toward
/// `to`. Mirrors the upstream
/// [`next-path` tests](https://github.com/zkochan/packages/blob/main/next-path/test.js)
/// without depending on the npm package.
#[test]
fn next_path_walks_one_segment_toward_target() {
    assert_eq!(
        next_path(Path::new("/"), Path::new("/Volumes/src/proj")),
        PathBuf::from("/Volumes"),
    );
    assert_eq!(
        next_path(Path::new("/Volumes"), Path::new("/Volumes/src/proj")),
        PathBuf::from("/Volumes/src"),
    );
    assert_eq!(
        next_path(Path::new("/Volumes/src"), Path::new("/Volumes/src/proj")),
        PathBuf::from("/Volumes/src/proj"),
    );
}

/// `next_path` returns `from` unchanged when `from` is not actually an
/// ancestor of `to` — keeps the loop in `root_link_target` from
/// looping forever on malformed inputs.
#[test]
fn next_path_returns_from_when_not_an_ancestor() {
    assert_eq!(
        next_path(Path::new("/Volumes/src"), Path::new("/Users/zoltan")),
        PathBuf::from("/Volumes/src"),
    );
}

/// `filesystem_root` extracts the root prefix of an absolute path —
/// `/` on Unix; the windows-prefix test is gated separately.
#[test]
#[cfg(unix)]
fn filesystem_root_unix_is_slash() {
    assert_eq!(filesystem_root(Path::new("/Volumes/src/proj")), PathBuf::from("/"));
    assert_eq!(filesystem_root(Path::new("/")), PathBuf::from("/"));
}

#[test]
#[cfg(windows)]
fn filesystem_root_windows_keeps_drive_prefix() {
    assert_eq!(filesystem_root(Path::new(r"C:\Users\proj")), PathBuf::from(r"C:\"));
}

/// Real-fixture happy path: project and home are on the same volume
/// (both inside the same `tempdir`), so the Host impl observes a
/// successful hardlink and `resolve_store_dir` returns the
/// `home_default` unchanged. This is the dominant branch on a single-
/// volume developer setup and the one the `SmartDefault` used to
/// short-circuit to without checking.
#[test]
fn resolve_store_dir_same_volume_uses_home_default() {
    use crate::api::Host;

    let tmp = tempdir().expect("create tempdir");
    let pkg_root = tmp.path().join("project");
    let pnpm_home = tmp.path().join("home/pnpm");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    fs::create_dir_all(&pnpm_home).expect("create home dir");
    let home_default = pnpm_home.join("store");

    let resolved = resolve_store_dir::<Host>(home_default.clone(), &pnpm_home, &pkg_root);
    assert_eq!(resolved, home_default);
}

/// Fake [`LinkProbe`] driven by a path-allowlist for the
/// `to_dir`: only directories whose canonical path starts with one
/// of the recorded prefixes accept the hardlink. Lets a test pin
/// the mountpoint deterministically without needing two real
/// volumes.
///
/// The allowlist lives in `ALLOW_PREFIXES`. To keep two
/// concurrently-running scenarios from clobbering each other's
/// allowlist between `set_allow` and the subsequent `resolve_store_dir`
/// call (nextest runs tests in parallel by default), every scenario
/// goes through [`PrefixProbe::with_allow`], which holds
/// [`PREFIX_PROBE_SCENARIO_LOCK`] across the entire set-and-probe.
/// Per `CodeRabbit` review on pnpm/pnpm#11804.
#[cfg(unix)]
static ALLOW_PREFIXES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

#[cfg(unix)]
static PREFIX_PROBE_SCENARIO_LOCK: Mutex<()> = Mutex::new(());

#[cfg(unix)]
struct PrefixProbe;

#[cfg(unix)]
impl PrefixProbe {
    /// Install `prefixes` as the allowlist, run `body`, then drop
    /// the scenario lock. Serialises every `PrefixProbe`-driven
    /// scenario so parallel test execution can't race the allowlist
    /// out from under a `resolve_store_dir` call.
    fn with_allow<Output>(prefixes: &[&Path], body: impl FnOnce() -> Output) -> Output {
        let _scenario =
            PREFIX_PROBE_SCENARIO_LOCK.lock().expect("PREFIX_PROBE_SCENARIO_LOCK not poisoned");
        let mut slot = ALLOW_PREFIXES.lock().expect("ALLOW_PREFIXES not poisoned");
        slot.clear();
        slot.extend(prefixes.iter().map(|p| p.to_path_buf()));
        drop(slot);
        body()
    }
}

#[cfg(unix)]
impl LinkProbe for PrefixProbe {
    fn can_link_between_dirs(_from_dir: &Path, to_dir: &Path) -> bool {
        let slot = ALLOW_PREFIXES.lock().expect("ALLOW_PREFIXES not poisoned");
        slot.iter().any(|allowed| to_dir.starts_with(allowed))
    }
}

/// Cross-volume scenario: home volume is unreachable, so the
/// algorithm walks from `/` toward `pkg_root` until it hits the
/// project's mount point (here a fake `/Volumes/src`), then returns
/// `<mountpoint>/.pnpm-store`. Mirrors pnpm's
/// [`'a link can be created to the a subdir in the root of the
/// drive'`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/test/index.ts#L84-L88)
/// test.
#[test]
#[cfg(unix)]
fn resolve_store_dir_cross_volume_walks_to_mountpoint() {
    let tmp = tempdir().expect("create tempdir");
    let mount = tmp.path().join("Volumes/src");
    let pkg_root = mount.join("project");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    // pkg_root must canonicalize, so symlinks (`/var` → `/private/var`
    // on macOS) don't surprise the prefix match.
    let pkg_root_canon = fs::canonicalize(&pkg_root).expect("canonicalize pkg_root");
    let mount_canon = pkg_root_canon.parent().expect("project has parent").to_path_buf();
    let home_default = PathBuf::from("/home/test-user/Library/pnpm/store");
    let pnpm_home = PathBuf::from("/home/test-user/Library/pnpm");

    // Only the mount and its descendants are linkable — anything
    // higher (the tempdir root, `/`, the home dir) fails the probe.
    let resolved = PrefixProbe::with_allow(&[&mount_canon], || {
        resolve_store_dir::<PrefixProbe>(home_default, &pnpm_home, &pkg_root_canon)
    });
    assert_eq!(resolved, mount_canon.join(".pnpm-store"));
}

/// When the algorithm walks toward `pkg_root` and the *parent* of the
/// first linkable directory is also linkable, prefer the parent.
/// Mirrors pnpm's `mountpointParent` short-walk at
/// [`index.ts:60-64`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L60-L64)
/// — the macOS case where `/Volumes` is writable even though the
/// volume mount is `/Volumes/src`.
#[test]
#[cfg(unix)]
fn resolve_store_dir_prefers_parent_when_parent_is_also_linkable() {
    let tmp = tempdir().expect("create tempdir");
    let parent_mount = tmp.path().join("VolumesGroup");
    let mount = parent_mount.join("src");
    let pkg_root = mount.join("project");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    let pkg_root_canon = fs::canonicalize(&pkg_root).expect("canonicalize pkg_root");
    let mount_canon = pkg_root_canon.parent().expect("project has parent").to_path_buf();
    let parent_canon = mount_canon.parent().expect("mount has parent").to_path_buf();
    let home_default = PathBuf::from("/home/test-user/Library/pnpm/store");
    let pnpm_home = PathBuf::from("/home/test-user/Library/pnpm");

    // Both the mount and its parent accept the link → algorithm
    // prefers the parent.
    let resolved = PrefixProbe::with_allow(&[&parent_canon], || {
        resolve_store_dir::<PrefixProbe>(home_default, &pnpm_home, &pkg_root_canon)
    });
    assert_eq!(resolved, parent_canon.join(".pnpm-store"));
}

/// When the only directory that accepts the hardlink is `pkg_root`
/// itself (i.e. linkability is confined to the project folder), the
/// algorithm falls back to the home default — putting the store
/// *inside* the project would be wrong. Pnpm makes the same call at
/// [`index.ts:67-68`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L67-L68).
#[test]
#[cfg(unix)]
fn resolve_store_dir_falls_back_when_only_pkg_root_is_linkable() {
    let tmp = tempdir().expect("create tempdir");
    let pkg_root = tmp.path().join("project");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    let pkg_root_canon = fs::canonicalize(&pkg_root).expect("canonicalize pkg_root");
    let home_default = PathBuf::from("/home/test-user/Library/pnpm/store");
    let pnpm_home = PathBuf::from("/home/test-user/Library/pnpm");

    let resolved = PrefixProbe::with_allow(&[&pkg_root_canon], || {
        resolve_store_dir::<PrefixProbe>(home_default.clone(), &pnpm_home, &pkg_root_canon)
    });
    assert_eq!(resolved, home_default);
}

/// When *nothing* on the way from filesystem root to `pkg_root` is
/// linkable (e.g. the volume holding `pkg_root` rejects every probe),
/// the algorithm cannot identify a mount point and falls back to
/// home. Matches pnpm's outer `try`/`catch` at
/// [`index.ts:56-77`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L56-L77):
/// any algorithm failure collapses to `storeInHomeDir`.
#[test]
#[cfg(unix)]
fn resolve_store_dir_falls_back_when_no_mountpoint_is_linkable() {
    let tmp = tempdir().expect("create tempdir");
    let pkg_root = tmp.path().join("project");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    let pkg_root_canon = fs::canonicalize(&pkg_root).expect("canonicalize pkg_root");
    let home_default = PathBuf::from("/home/test-user/Library/pnpm/store");
    let pnpm_home = PathBuf::from("/home/test-user/Library/pnpm");

    // Empty allowlist — every probe returns false.
    let resolved = PrefixProbe::with_allow(&[], || {
        resolve_store_dir::<PrefixProbe>(home_default.clone(), &pnpm_home, &pkg_root_canon)
    });
    assert_eq!(resolved, home_default);
}

/// `pkg_root` that doesn't exist on disk (e.g. CLI run before
/// `mkdir -p`) cannot be canonicalized, so the algorithm falls
/// back to home. The store creation code downstream will create the
/// directory itself, so a missing `pkg_root` is not an error here.
#[test]
fn resolve_store_dir_falls_back_when_pkg_root_does_not_exist() {
    use crate::api::Host;

    let home_default = PathBuf::from("/home/test-user/Library/pnpm/store");
    let pnpm_home = PathBuf::from("/home/test-user/Library/pnpm");
    let missing = PathBuf::from("/this/path/should/not/exist/anywhere");
    let resolved = resolve_store_dir::<Host>(home_default.clone(), &pnpm_home, &missing);
    assert_eq!(resolved, home_default);
}

/// `host_can_link_between_dirs` returns `true` between two directories
/// on the same tempdir-volume (the common case). This is the
/// production primitive [`Host`][crate::api::Host] threads through the
/// [`LinkProbe`] trait; same-volume happiness is exercised here so a
/// future refactor that broke the linker would be caught directly.
#[test]
fn host_can_link_between_dirs_same_volume_is_true() {
    let tmp = tempdir().expect("create tempdir");
    let from = tmp.path().join("from");
    let to = tmp.path().join("to");
    fs::create_dir_all(&from).expect("create from");
    fs::create_dir_all(&to).expect("create to");
    assert!(host_can_link_between_dirs(&from, &to));
}

/// `host_can_link_between_dirs` collapses every failure mode to
/// `false`, including the case where `from_dir` does not exist
/// (so the temp source file can't be created). Mirrors pnpm's
/// `canLink` returning `false` on `EACCES` / `EPERM` / `EXDEV` /
/// anything else — pacquet's probe widens that to "any error means
/// not linkable" so the algorithm degrades to `home_default` rather
/// than aborting the install.
#[test]
fn host_can_link_between_dirs_missing_from_dir_is_false() {
    let tmp = tempdir().expect("create tempdir");
    let missing_from = tmp.path().join("does/not/exist");
    let to = tmp.path().join("to");
    fs::create_dir_all(&to).expect("create to");
    assert!(!host_can_link_between_dirs(&missing_from, &to));
}
