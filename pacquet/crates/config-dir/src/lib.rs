use std::path::{Path, PathBuf};

/// Resolve the directory pnpm reads its global `config.yaml` from,
/// for an application that follows pnpm's config-dir convention under
/// its own `app_name` leaf (`"pnpm"` for pnpm/pacquet, `"pnpr"` for
/// the registry server).
///
/// Port of pnpm's
/// [`getConfigDir`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/dirs.ts#L67-L86).
/// Resolution order:
///
/// 1. `$XDG_CONFIG_HOME/<app_name>`;
/// 2. Windows: `%LOCALAPPDATA%/<app_name>/config` (falling back to
///    `~/.config/<app_name>` when `LOCALAPPDATA` is unset);
/// 3. macOS: `~/Library/Preferences/<app_name>`;
/// 4. other: `~/.config/<app_name>`.
///
/// Returns `None` only when the home directory is unavailable and the
/// env vars that bypass it are unset — the caller treats that as "no
/// global config."
///
/// `os`, the env values, and `home` are passed in rather than read
/// from the process so callers keep their own environment seam and
/// every branch is unit-testable without mutating process state.
/// `os` is a [`std::env::consts::OS`] string (`"macos"`, `"windows"`,
/// `"linux"`, ...). `home` is a thunk so the (potentially I/O-bound)
/// home-dir lookup is skipped whenever an env var short-circuits the
/// resolution.
pub fn config_dir(
    app_name: &str,
    os: &str,
    xdg_config_home: Option<&str>,
    local_app_data: Option<&str>,
    home: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(xdg_config_home) = xdg_config_home {
        return Some(Path::new(xdg_config_home).join(app_name));
    }
    if os == "windows"
        && let Some(local_app_data) = local_app_data
    {
        return Some(Path::new(local_app_data).join(app_name).join("config"));
    }
    let home = home()?;
    Some(match os {
        "macos" => home.join("Library").join("Preferences").join(app_name),
        _ => home.join(".config").join(app_name),
    })
}

#[cfg(test)]
mod tests {
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
            // The home thunk panics: the XDG branch must short-circuit
            // before it is ever called.
            let dir = config_dir("pnpm", os, Some("/srv/xdg"), Some("C:\\LocalAppData"), || {
                unreachable!("home must not be consulted when XDG_CONFIG_HOME is set")
            });
            assert_eq!(dir, Some(PathBuf::from("/srv/xdg").join("pnpm")), "{os}");
        }
    }

    #[test]
    fn macos_uses_library_preferences() {
        let dir = config_dir("pnpr", "macos", None, None, home("/Users/u"));
        assert_eq!(
            dir,
            Some(Path::new("/Users/u").join("Library").join("Preferences").join("pnpr")),
        );
    }

    #[test]
    fn linux_uses_dot_config() {
        let dir = config_dir("pnpr", "linux", None, None, home("/home/u"));
        assert_eq!(dir, Some(Path::new("/home/u").join(".config").join("pnpr")));
    }

    #[test]
    fn windows_uses_local_app_data() {
        let dir = config_dir(
            "pnpm",
            "windows",
            None,
            Some("C:\\Users\\u\\AppData\\Local"),
            home("C:\\Users\\u"),
        );
        assert_eq!(
            dir,
            Some(Path::new("C:\\Users\\u\\AppData\\Local").join("pnpm").join("config"))
        );
    }

    #[test]
    fn windows_without_local_app_data_falls_back_to_dot_config() {
        let dir = config_dir("pnpm", "windows", None, None, home("C:\\Users\\u"));
        assert_eq!(dir, Some(Path::new("C:\\Users\\u").join(".config").join("pnpm")));
    }

    #[test]
    fn none_when_home_missing_and_env_bypass_unset() {
        assert!(config_dir("pnpm", "linux", None, None, no_home).is_none());
    }

    #[test]
    fn app_name_is_the_leaf_on_every_branch() {
        assert!(
            config_dir("pnpr", "linux", None, None, home("/home/u")).unwrap().ends_with("pnpr")
        );
        assert!(
            config_dir("pnpm", "linux", None, None, home("/home/u")).unwrap().ends_with("pnpm")
        );
    }
}
