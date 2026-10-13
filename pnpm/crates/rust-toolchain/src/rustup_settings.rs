use std::path::{Path, PathBuf};

/// Where rustup keeps its settings, such as the directory overrides
/// `rustup override set` records and the default host triple.
#[must_use]
pub fn rustup_settings_file() -> Option<PathBuf> {
    let rustup_home = std::env::var_os("RUSTUP_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| pnpm_config::home_dir().map(|home| home.join(".rustup")))?;
    Some(rustup_home.join("settings.toml"))
}

/// The host triple rustup installs toolchains for, as `rustup set
/// default-host` records it.
pub(crate) fn rustup_default_host_triple() -> Option<String> {
    default_host_triple_in(&rustup_settings_file()?)
}

fn default_host_triple_in(settings: &Path) -> Option<String> {
    let settings: toml::Table = std::fs::read_to_string(settings)
        .ok()?
        .parse()
        .ok()?;
    Some(
        settings
            .get("default_host_triple")?
            .as_str()?
            .to_string(),
    )
}

#[cfg(test)]
mod tests;
