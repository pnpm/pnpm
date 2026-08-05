use super::config_dir;
use std::path::{Path, PathBuf};

fn home(path: &str) -> impl FnOnce() -> Option<PathBuf> + use<'_> {
    move || Some(PathBuf::from(path))
}

fn no_home() -> Option<PathBuf> {
    None
}

#[test]
fn prefers_xdg_config_home_on_every_os_without_consulting_home() {
    for os in ["linux", "macos", "windows"] {
        let dir = config_dir("pnpm", os, Some("/srv/xdg"), Some(r"C:\LocalAppData"), || {
            unreachable!("home must not be consulted when XDG_CONFIG_HOME is set")
        });
        assert_eq!(dir, Some(PathBuf::from("/srv/xdg").join("pnpm")), "{os}");
    }
}

#[test]
fn macos_uses_library_preferences() {
    let dir = config_dir("pnpr", "macos", None, None, home("/Users/u"));
    assert_eq!(dir, Some(Path::new("/Users/u").join("Library").join("Preferences").join("pnpr")));
}

#[test]
fn linux_uses_dot_config() {
    let dir = config_dir("pnpr", "linux", None, None, home("/home/u"));
    assert_eq!(dir, Some(Path::new("/home/u").join(".config").join("pnpr")));
}

#[test]
fn windows_uses_local_app_data() {
    let dir =
        config_dir("pnpm", "windows", None, Some(r"C:\Users\u\AppData\Local"), home(r"C:\Users\u"));
    assert_eq!(dir, Some(Path::new(r"C:\Users\u\AppData\Local").join("pnpm").join("config")));
}

#[test]
fn windows_without_local_app_data_falls_back_to_dot_config() {
    let dir = config_dir("pnpm", "windows", None, None, home(r"C:\Users\u"));
    assert_eq!(dir, Some(Path::new(r"C:\Users\u").join(".config").join("pnpm")));
}

#[test]
fn none_when_home_missing_and_env_bypass_unset() {
    assert!(config_dir("pnpm", "linux", None, None, no_home).is_none());
}
