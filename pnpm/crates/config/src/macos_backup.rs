use crate::SmartDefault;

#[derive(Debug, Clone, SmartDefault)]
pub struct MacosBackupConfig {
    #[default = false]
    pub exclude_modules_dir: bool,
    #[default = false]
    pub exclude_store_dir: bool,
}
