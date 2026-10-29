use crate::SmartDefault;

/// Machine-local macOS Time Machine policy. Both directories remain eligible
/// for backup unless the user opts out in the global pnpm configuration or
/// through `PNPM_CONFIG_*` environment variables.
#[derive(Debug, Clone, SmartDefault)]
pub struct MacosBackupConfig {
    #[default = false]
    pub exclude_modules_dir: bool,
    #[default = false]
    pub exclude_store_dir: bool,
}
