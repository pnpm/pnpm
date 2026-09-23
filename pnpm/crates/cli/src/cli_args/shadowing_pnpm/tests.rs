use super::{InstallOrigin, ShadowingPnpm, find_shadowing_pnpm};
use pretty_assertions::assert_eq;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

/// The file name a `PATH` lookup for `pnpm` accepts on this platform.
const PNPM: &str = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };

fn write_executable(dir: &Path, name: &str, contents: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

fn path_env(dirs: &[&Path]) -> OsString {
    std::env::join_paths(dirs).unwrap()
}

#[test]
fn nothing_shadows_without_a_path() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(find_shadowing_pnpm(&home.path().join("bin"), None), None);
}

#[test]
fn nothing_shadows_when_the_global_bin_comes_first() {
    let home = tempfile::tempdir().unwrap();
    let global_bin = home.path().join("bin");
    write_executable(&global_bin, PNPM, "");
    let other = home.path().join("other");
    write_executable(&other, PNPM, "");

    let path = path_env(&[&global_bin, &other]);
    assert_eq!(find_shadowing_pnpm(&global_bin, Some(&path)), None);
}

#[test]
fn nothing_shadows_when_no_pnpm_is_on_the_path() {
    let home = tempfile::tempdir().unwrap();
    let global_bin = home.path().join("bin");
    let empty = home.path().join("empty");
    fs::create_dir_all(&empty).unwrap();

    let path = path_env(&[&empty]);
    assert_eq!(find_shadowing_pnpm(&global_bin, Some(&path)), None);
}

#[test]
fn a_pnpm_ahead_of_the_global_bin_shadows_it() {
    let home = tempfile::tempdir().unwrap();
    let global_bin = home.path().join("bin");
    write_executable(&global_bin, PNPM, "");
    let other = home.path().join("other");
    let executable = write_executable(&other, PNPM, "");

    let path = path_env(&[&other, &global_bin]);
    let shadowing = find_shadowing_pnpm(&global_bin, Some(&path));
    dbg!(&shadowing);
    assert_eq!(
        shadowing,
        Some(ShadowingPnpm {
            executable,
            origin: InstallOrigin::Unknown,
            global_bin_on_path: true
        }),
    );
}

#[test]
fn a_pnpm_shadows_a_global_bin_that_is_not_on_the_path_yet() {
    let home = tempfile::tempdir().unwrap();
    let global_bin = home.path().join("bin");
    let other = home.path().join("other");
    let executable = write_executable(&other, PNPM, "");

    let path = path_env(&[&other]);
    let shadowing = find_shadowing_pnpm(&global_bin, Some(&path));
    dbg!(&shadowing);
    assert_eq!(
        shadowing,
        Some(ShadowingPnpm {
            executable,
            origin: InstallOrigin::Unknown,
            global_bin_on_path: false
        }),
    );
}

/// The v10 layout links `pnpm` straight into the pnpm home directory, and
/// CI actions do the same; that is pnpm's own, not another installer's.
#[test]
fn a_pnpm_in_the_pnpm_home_directory_is_pnpm_itself() {
    let home = tempfile::tempdir().unwrap();
    let global_bin = home.path().join("bin");
    write_executable(home.path(), PNPM, "");

    let path = path_env(&[home.path()]);
    assert_eq!(find_shadowing_pnpm(&global_bin, Some(&path)), None);
}

#[cfg(unix)]
#[test]
fn a_symlink_into_the_global_bin_is_pnpm_itself() {
    let home = tempfile::tempdir().unwrap();
    let global_bin = home.path().join("bin");
    let executable = write_executable(&global_bin, PNPM, "");
    let links = home.path().join("links");
    fs::create_dir_all(&links).unwrap();
    std::os::unix::fs::symlink(&executable, links.join(PNPM)).unwrap();

    let path = path_env(&[&links, &global_bin]);
    assert_eq!(find_shadowing_pnpm(&global_bin, Some(&path)), None);
}

#[test]
fn the_origin_is_read_off_the_executables_real_path() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let cases = [
        ("opt/homebrew/lib/node_modules/pnpm/bin/pnpm.mjs", InstallOrigin::NpmGlobal),
        ("opt/homebrew/Cellar/pnpm/12.6.0/bin/pnpm", InstallOrigin::Homebrew),
        ("usr/local/lib/node_modules/corepack/shims/pnpm", InstallOrigin::Corepack),
        ("home/me/.volta/bin/pnpm", InstallOrigin::Volta),
        ("Users/me/scoop/shims/pnpm.cmd", InstallOrigin::Scoop),
        ("home/me/bin/pnpm", InstallOrigin::Unknown),
    ];
    for (relative, expected) in cases {
        let executable = root.join(relative);
        let origin = InstallOrigin::detect(&executable);
        assert_eq!(origin, expected, "{relative}");
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_executable_is_classified_by_its_target() {
    let root = tempfile::tempdir().unwrap();
    let target = write_executable(&root.path().join("lib/node_modules/pnpm/bin"), "pnpm.mjs", "");
    let link_dir = root.path().join("bin");
    fs::create_dir_all(&link_dir).unwrap();
    let link = link_dir.join("pnpm");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    assert_eq!(InstallOrigin::detect(&link), InstallOrigin::NpmGlobal);
}

/// Windows shims are `.cmd` scripts in a directory that says nothing about
/// their origin (`%APPDATA%\npm`), so the script's target decides.
#[test]
fn a_shim_script_is_classified_by_its_contents() {
    let root = tempfile::tempdir().unwrap();
    let npm_shim = write_executable(
        &root.path().join("npm"),
        "pnpm.cmd",
        "@ECHO off\r\n\"%~dp0\\node.exe\" \"%~dp0\\node_modules\\pnpm\\bin\\pnpm.cjs\" %*\r\n",
    );
    assert_eq!(InstallOrigin::detect(&npm_shim), InstallOrigin::NpmGlobal);

    let corepack_shim = write_executable(
        &root.path().join("node"),
        "pnpm",
        "#!/bin/sh\nexec corepack pnpm \"$@\"\n",
    );
    assert_eq!(InstallOrigin::detect(&corepack_shim), InstallOrigin::Corepack);

    let plain = write_executable(&root.path().join("plain"), "pnpm", "#!/bin/sh\nexit 0\n");
    assert_eq!(InstallOrigin::detect(&plain), InstallOrigin::Unknown);
}

#[test]
fn the_warning_names_the_removal_command_and_the_path_order() {
    let shadowing = ShadowingPnpm {
        executable: PathBuf::from("/opt/homebrew/bin/pnpm"),
        origin: InstallOrigin::NpmGlobal,
        global_bin_on_path: true,
    };
    let warning = shadowing.warning(Path::new("/home/me/.local/share/pnpm/bin"));
    assert_eq!(
        warning,
        "\"pnpm\" on PATH is /opt/homebrew/bin/pnpm (installed with npm), which comes before /home/me/.local/share/pnpm/bin. \
         Your shell keeps running that pnpm, not the one pnpm installed to /home/me/.local/share/pnpm/bin. \
         To finish switching, run \"npm uninstall -g pnpm\" or move /home/me/.local/share/pnpm/bin ahead of /opt/homebrew/bin in PATH.",
    );
}

#[test]
fn the_warning_for_an_unknown_origin_only_reorders_the_path() {
    let shadowing = ShadowingPnpm {
        executable: PathBuf::from("/usr/local/bin/pnpm"),
        origin: InstallOrigin::Unknown,
        global_bin_on_path: false,
    };
    let warning = shadowing.warning(Path::new("/home/me/.local/share/pnpm/bin"));
    assert_eq!(
        warning,
        "\"pnpm\" on PATH is /usr/local/bin/pnpm (not installed by pnpm), and /home/me/.local/share/pnpm/bin is not on PATH yet. \
         Once a new shell adds it, it has to come first: move /home/me/.local/share/pnpm/bin ahead of /usr/local/bin in PATH.",
    );
}
