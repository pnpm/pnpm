use super::{Path, PathBuf, config_file::UiFile, resolve_relative};

/// The web UI pnpr serves at `/-/ui/`.
#[derive(Debug, Clone)]
pub struct UiConfig {
    /// `false` serves no UI, even when one is installed. Defaults to `true`.
    pub enabled: bool,
    /// The built UI to serve. `None` looks for the `@pnpm/pnpr-ui` package
    /// installed next to pnpr.
    pub dir: Option<PathBuf>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { enabled: true, dir: None }
    }
}

pub(super) fn build_ui_config(file: UiFile, base_dir: &Path) -> UiConfig {
    UiConfig { enabled: file.enabled, dir: file.dir.map(|dir| resolve_relative(&dir, base_dir)) }
}
