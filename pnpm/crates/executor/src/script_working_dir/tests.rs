use super::{
    emulator_working_dir,
    is_refused_directory,
    shorter_working_dirs,
};
use std::{
    io,
    path::{
        Path,
        PathBuf,
    },
};

#[test]
fn the_emulator_keeps_a_short_working_directory() {
    let root = tempfile::Builder::new()
        .prefix("pnpm-wd-")
        .tempdir()
        .expect("create temporary directory");

    assert!(matches!(emulator_working_dir(root.path()), std::borrow::Cow::Borrowed(_)));
}

#[test]
#[cfg(windows)]
fn the_emulator_measures_a_working_directory_in_utf16_code_units() {
    use std::os::windows::ffi::OsStrExt;

    let root = tempfile::Builder::new()
        .prefix("pnpm-wd-")
        .tempdir()
        .expect("create temporary directory");
    let mut component = String::new();
    while root
        .path()
        .join(&component)
        .as_os_str()
        .len()
        <= 258
    {
        component.push('é');
    }
    let pkg_root = root.path().join(component);
    assert!(pkg_root.as_os_str().len() > 258);
    assert!(
        pkg_root
            .as_os_str()
            .encode_wide()
            .count()
            <= 258,
    );
    std::fs::create_dir(&pkg_root).expect("create Unicode package root");

    assert!(matches!(emulator_working_dir(&pkg_root), std::borrow::Cow::Borrowed(_)));
}

#[test]
fn the_normalized_spelling_comes_first() {
    let root = tempfile::Builder::new()
        .prefix("pnpm-wd-")
        .tempdir()
        .expect("create temporary directory");
    let slot = slot_dir(&root.path().join("workspace").join(".."));
    std::fs::create_dir_all(&slot).expect("create the deep slot");

    let spellings = shorter_working_dirs(&slot);
    let first = spellings.first().expect("a relative storeDir leaves a `..` to drop");
    assert!(
        !first
            .components()
            .any(|component| component.as_os_str() == ".."),
        "{first:?} still steps through a parent",
    );
    assert!(
        first.as_os_str().len() < slot.as_os_str().len(),
        "{first:?} must be shorter than {slot:?}",
    );
}

#[test]
fn every_fallback_is_shorter_than_the_original() {
    let root = tempfile::Builder::new()
        .prefix("pnpm-wd-")
        .tempdir()
        .expect("create temporary directory");
    let slot = slot_dir(root.path());
    std::fs::create_dir_all(&slot).expect("create the deep slot");

    for spelling in shorter_working_dirs(&slot) {
        assert!(
            spelling.as_os_str().len() < slot.as_os_str().len(),
            "{spelling:?} is no shorter than {slot:?}",
        );
    }
}

#[test]
fn only_windows_reports_error_directory_as_a_refused_working_directory() {
    let refused = io::Error::from_raw_os_error(267);
    assert_eq!(is_refused_directory(&refused), cfg!(windows));
    assert!(!is_refused_directory(&io::Error::from_raw_os_error(2)));
}

#[test]
#[cfg_attr(not(windows), ignore = "only Windows bounds a working directory")]
fn a_slot_windows_refuses_spawns_in_a_shorter_spelling() {
    let root = tempfile::Builder::new()
        .prefix("pnpm-wd-")
        .tempdir()
        .expect("create temporary directory");
    let slot = slot_dir(&root.path().join("workspace").join(".."));
    std::fs::create_dir_all(&slot).expect("create the deep slot");

    let refusal = spawn_in(&slot).expect_err("Windows must refuse a working directory this long");
    assert!(is_refused_directory(&refusal), "expected ERROR_DIRECTORY, got {refusal:?}");

    let working_dir = emulator_working_dir(&slot);
    spawn_in(&working_dir).unwrap_or_else(|error| {
        panic!(
            "the emulator spelling of a {}-character slot was refused ({} characters): {error:?}",
            slot.as_os_str().len(),
            working_dir.as_os_str().len(),
        )
    });
}

#[cfg(windows)]
fn spawn_in(dir: &Path) -> io::Result<std::process::ExitStatus> {
    std::process::Command::new("cmd")
        .args(["/d", "/s", "/c", "cd"])
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .status()
}

#[cfg(not(windows))]
fn spawn_in(_dir: &Path) -> io::Result<std::process::ExitStatus> {
    unreachable!("the test that calls this is ignored off Windows")
}

fn slot_dir(base: &Path) -> PathBuf {
    let mut dir = base
        .join("v11")
        .join("links")
        .join("@pnpm.e2e")
        .join("pre-and-postinstall-scripts-example")
        .join("1.0.0")
        .join("18ee99614ef3696a0b10d1d9893d9ec41c393462eec96e154981c3d9cca0c268")
        .join("node_modules")
        .join("@pnpm.e2e")
        .join("pre-and-postinstall-scripts-example");
    while dir.as_os_str().len() <= 260 {
        dir = dir.join("p");
    }
    dir
}
