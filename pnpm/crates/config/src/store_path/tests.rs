use super::{filesystem_root, host_can_link_between_dirs, next_path, resolve_store_dir};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

// `LinkProbe` is shared by the Windows regression test and the Unix-only
// `prefix_probe!` fake below. Only the fake's allowlist needs `Mutex`, so
// that import remains Unix-gated.
use crate::api::LinkProbe;
#[cfg(unix)]
use std::sync::Mutex;

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

#[test]
#[cfg_attr(not(windows), ignore = "requires Windows path canonicalization")]
fn resolve_store_dir_cross_volume_uses_project_drive_without_verbatim_prefix() {
    struct RootProbe;
    impl LinkProbe for RootProbe {
        fn can_link_between_dirs(from_dir: &Path, to_dir: &Path) -> bool {
            to_dir == filesystem_root(from_dir)
        }
    }

    let tmp = tempdir().expect("create tempdir");
    let pkg_root = tmp.path().join("project");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    let project_drive = filesystem_root(&pkg_root);
    let pnpm_home = if project_drive.to_string_lossy().eq_ignore_ascii_case(r"C:\") {
        PathBuf::from(r"D:\pnpm-home")
    } else {
        PathBuf::from(r"C:\pnpm-home")
    };
    let home_default = pnpm_home.join("store");
    let expected = project_drive.join(".pnpm-store");

    let resolved = resolve_store_dir::<RootProbe>(home_default, &pnpm_home, &pkg_root);
    assert_eq!(resolved, expected);
    let resolved_display = resolved.display().to_string();
    assert!(
        !resolved_display.starts_with(r"\\?\"),
        "resolved store dir has a verbatim prefix: {resolved_display}",
    );
}

// Per-test [`LinkProbe`] fake whose `can_link_between_dirs` accepts a `to_dir`
// only under an allowlisted prefix, pinning the mountpoint deterministically
// without two real volumes. The allowlist is fn-local, so each `#[test]` owns
// its own and concurrent tests never share it.
#[cfg(unix)]
macro_rules! prefix_probe {
    () => {
        static ALLOW_PREFIXES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

        struct PrefixProbe;
        impl LinkProbe for PrefixProbe {
            fn can_link_between_dirs(_from_dir: &Path, to_dir: &Path) -> bool {
                ALLOW_PREFIXES
                    .lock()
                    .expect("ALLOW_PREFIXES not poisoned")
                    .iter()
                    .any(|allowed| to_dir.starts_with(allowed))
            }
        }

        fn set_allow(prefixes: &[&Path]) {
            let mut slot = ALLOW_PREFIXES.lock().expect("ALLOW_PREFIXES not poisoned");
            slot.clear();
            slot.extend(prefixes.iter().map(|prefix| prefix.to_path_buf()));
        }
    };
}

#[test]
#[cfg(unix)]
fn resolve_store_dir_cross_volume_walks_to_mountpoint() {
    prefix_probe!();
    let tmp = tempdir().expect("create tempdir");
    let mount = tmp.path().join("Volumes/src");
    let pkg_root = mount.join("project");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    // pkg_root must canonicalize, so symlinks (`/var` → `/private/var`
    // on macOS) don't surprise the prefix match.
    let pkg_root_canon = fs::canonicalize(&pkg_root).expect("canonicalize pkg_root");
    let mount_canon = pkg_root_canon
        .parent()
        .expect("project has parent")
        .to_path_buf();
    let home_default = PathBuf::from("/home/test-user/Library/pnpm/store");
    let pnpm_home = PathBuf::from("/home/test-user/Library/pnpm");

    // Only the mount and its descendants are linkable — anything
    // higher (the tempdir root, `/`, the home dir) fails the probe.
    set_allow(&[&mount_canon]);
    let resolved = resolve_store_dir::<PrefixProbe>(home_default, &pnpm_home, &pkg_root_canon);
    assert_eq!(resolved, mount_canon.join(".pnpm-store"));
}

#[test]
#[cfg(unix)]
fn resolve_store_dir_prefers_parent_when_parent_is_also_linkable() {
    prefix_probe!();
    let tmp = tempdir().expect("create tempdir");
    let parent_mount = tmp.path().join("VolumesGroup");
    let mount = parent_mount.join("src");
    let pkg_root = mount.join("project");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    let pkg_root_canon = fs::canonicalize(&pkg_root).expect("canonicalize pkg_root");
    let mount_canon = pkg_root_canon
        .parent()
        .expect("project has parent")
        .to_path_buf();
    let parent_canon = mount_canon
        .parent()
        .expect("mount has parent")
        .to_path_buf();
    let home_default = PathBuf::from("/home/test-user/Library/pnpm/store");
    let pnpm_home = PathBuf::from("/home/test-user/Library/pnpm");

    set_allow(&[&parent_canon]);
    let resolved = resolve_store_dir::<PrefixProbe>(home_default, &pnpm_home, &pkg_root_canon);
    assert_eq!(resolved, parent_canon.join(".pnpm-store"));
}

#[test]
#[cfg(unix)]
fn resolve_store_dir_uses_node_modules_when_only_pkg_root_is_linkable() {
    prefix_probe!();
    let tmp = tempdir().expect("create tempdir");
    let pkg_root = tmp.path().join("project");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    let pkg_root_canon = fs::canonicalize(&pkg_root).expect("canonicalize pkg_root");
    let home_default = PathBuf::from("/home/test-user/Library/pnpm/store");
    let pnpm_home = PathBuf::from("/home/test-user/Library/pnpm");

    set_allow(&[&pkg_root_canon]);
    let resolved = resolve_store_dir::<PrefixProbe>(home_default, &pnpm_home, &pkg_root_canon);
    assert_eq!(resolved, pkg_root_canon.join("node_modules").join(".pnpm-store"));
}

#[test]
#[cfg(unix)]
fn resolve_store_dir_falls_back_when_no_mountpoint_is_linkable() {
    prefix_probe!();
    let tmp = tempdir().expect("create tempdir");
    let pkg_root = tmp.path().join("project");
    fs::create_dir_all(&pkg_root).expect("create project dir");
    let pkg_root_canon = fs::canonicalize(&pkg_root).expect("canonicalize pkg_root");
    let home_default = PathBuf::from("/home/test-user/Library/pnpm/store");
    let pnpm_home = PathBuf::from("/home/test-user/Library/pnpm");

    set_allow(&[]);
    let resolved =
        resolve_store_dir::<PrefixProbe>(home_default.clone(), &pnpm_home, &pkg_root_canon);
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

/// Set up `<tmp>/home` as the pnpm home and `<tmp>/volume/project` as a
/// project whose volume is `<tmp>/volume`, then resolve the default store
/// with a [`LinkProbe`] that only links within that volume.
#[cfg(unix)]
macro_rules! resolve_relocated_store {
    ($tmp:ident => $config:ident, $root:ident, $home_store:ident) => {
        prefix_probe!();
        impl crate::api::GetHomeDir for PrefixProbe {
            fn home_dir() -> Option<PathBuf> {
                Some(PathBuf::from("/home/test-user"))
            }
        }

        let $root = fs::canonicalize($tmp.path()).expect("canonicalize tempdir");
        let $home_store = $root.join("home/store").join(pnpm_store_dir::STORE_VERSION);
        fs::create_dir_all($root.join("volume/project")).expect("create project dir");
        set_allow(&[&$root.join("volume")]);
        let mut config = crate::Config::new();
        config.resolve_store_dir_from_home::<PrefixProbe>(
            &$root.join("home"),
            &$root.join("volume/project"),
        );
        let $config = config;
    };
}

#[test]
#[cfg(unix)]
fn bypassed_home_store_warning_names_both_stores_when_home_store_exists() {
    let tmp = tempdir().expect("create tempdir");
    resolve_relocated_store!(tmp => config, root, home_store);
    fs::create_dir_all(&home_store).expect("create home store");
    let relocated = root.join("volume/.pnpm-store").join(pnpm_store_dir::STORE_VERSION);

    assert_eq!(config.store_dir.root(), relocated);
    assert_eq!(
        config.bypassed_home_store_warning(),
        Some(format!(
            "The store at {} is not used because packages cannot be hard linked from it into this project. Using the store at {} instead. Set storeDir to choose the store.",
            home_store.display(),
            relocated.display(),
        )),
    );
}

#[test]
#[cfg(unix)]
fn bypassed_home_store_warning_is_none_without_a_home_store() {
    let tmp = tempdir().expect("create tempdir");
    resolve_relocated_store!(tmp => config, root, home_store);
    assert!(config.store_relocation.is_some());
    assert!(!home_store.exists(), "{} must not exist", root.display());
    assert_eq!(config.bypassed_home_store_warning(), None);
}

#[test]
#[cfg(unix)]
fn bypassed_home_store_warning_is_none_after_an_explicit_store_dir() {
    let tmp = tempdir().expect("create tempdir");
    resolve_relocated_store!(tmp => config, root, home_store);
    fs::create_dir_all(&home_store).expect("create home store");
    let mut config = config;
    config.store_dir = root.join("explicit-store").into();
    assert_eq!(config.bypassed_home_store_warning(), None);
}

#[test]
#[cfg(unix)]
fn bypassed_home_store_warning_is_none_when_the_home_store_is_linkable() {
    let tmp = tempdir().expect("create tempdir");
    resolve_relocated_store!(tmp => config, root, home_store);
    fs::create_dir_all(&home_store).expect("create home store");
    let mut config = config;
    set_allow(&[&root]);
    config.resolve_store_dir_from_home::<PrefixProbe>(
        &root.join("home"),
        &root.join("volume/project"),
    );
    assert_eq!(config.store_dir.root(), home_store);
    assert_eq!(config.store_relocation, None);
    assert_eq!(config.bypassed_home_store_warning(), None);
}
