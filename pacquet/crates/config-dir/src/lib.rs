use std::path::{Path, PathBuf};

/// Resolve the directory pnpm reads its global `config.yaml` from,
/// for an application that follows pnpm's config-dir convention under
/// its own `app_name` leaf (`"pnpm"` for pnpm/pacquet, `"pnpr"` for
/// the registry server).
///
/// Port of pnpm's
/// [`getConfigDir`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/dirs.ts#L67-L86).
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
mod tests;
