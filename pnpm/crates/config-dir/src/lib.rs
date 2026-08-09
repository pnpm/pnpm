use std::path::{Path, PathBuf};

/// Resolve the directory pnpm reads its global `config.yaml` from,
/// for an application that follows pnpm's config-dir convention under
/// its own `app_name` leaf (`"pnpm"` for pnpm/pacquet, `"pnpr"` for
/// the registry server).
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

/// Resolve the machine-local state directory for `app_name`, mirroring
/// pnpm's `getStateDir`: `$XDG_STATE_HOME/<app>`, else
/// `~/.local/state/<app>` on non-Windows, else
/// `%LOCALAPPDATA%\<app>-state`, else `~/.<app>-state`. Same
/// environment-seam shape as [`config_dir`].
pub fn state_dir(
    app_name: &str,
    os: &str,
    xdg_state_home: Option<&str>,
    local_app_data: Option<&str>,
    home: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    // Empty or relative env values are treated as unset: resolving them
    // would place mutable machine state under whatever the invocation's
    // working directory happens to be.
    if let Some(xdg_state_home) = xdg_state_home.filter(|value| Path::new(value).is_absolute()) {
        return Some(Path::new(xdg_state_home).join(app_name));
    }
    if os != "windows" {
        return Some(home()?.join(".local").join("state").join(app_name));
    }
    if let Some(local_app_data) = local_app_data.filter(|value| Path::new(value).is_absolute()) {
        return Some(Path::new(local_app_data).join(format!("{app_name}-state")));
    }
    Some(home()?.join(format!(".{app_name}-state")))
}

#[cfg(test)]
mod tests;
