use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_registry_mock::AutoMockInstance;
use std::{fs, path::PathBuf, process::Command};
use tempfile::{TempDir, tempdir};
use text_block_macros::text_block_fnl;

/// Assets for an integration test involving spawning `pacquet` and/or `pnpm` as
/// sub-process(es) in a temporary directory.
pub struct CommandTempCwd<NpmrcInfo> {
    /// Command of `pacquet` with [`Self::workspace`] as working directory.
    pub pacquet: Command,
    /// Command of `pnpm` with [`Self::workspace`] as working directory.
    pub pnpm: Command,
    /// Temporary directory that contains all other paths.
    pub root: TempDir,
    /// The `workspace` sub-directory.
    pub workspace: PathBuf,
    /// Optional info regarding the creation of `.npmrc`.
    pub npmrc_info: NpmrcInfo,
}

impl CommandTempCwd<()> {
    /// Create a temporary directory, a `workspace` sub-directory, a `pacquet` command,
    /// and a `pnpm` command with current dir set to the `workspace` sub-directory.
    pub fn init() -> Self {
        let root = tempdir().expect("create temporary directory");
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).expect("create temporary workspace for the commands");
        let pacquet = Command::cargo_bin("pacquet")
            .expect("find the pacquet binary")
            .with_current_dir(&workspace);
        let pnpm = Command::new("pnpm").with_current_dir(&workspace);
        CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info: () }
    }
}

/// Information after the creation of an `.npmrc` file and a mocked registry from assets provided by [`CommandTempCwd`].
#[must_use]
pub struct AddMockedRegistry {
    /// Path to the created `.npmrc` file.
    pub npmrc_path: PathBuf,
    /// Absolute path to the store directory as defined by the `.npmrc` file.
    pub store_dir: PathBuf,
    /// Absolute path to the cache directory as defined by the `.npmrc` file.
    pub cache_dir: PathBuf,
    /// Anchor to a mocked registry instance. The server will be stop when [dropped](Drop).
    pub mock_instance: AutoMockInstance,
}

impl CommandTempCwd<()> {
    /// Create a mock registry and a `.npmrc` file that defines `store-dir`, `cache-dir`, and `registry`.
    ///
    /// Also writes a `pnpm-workspace.yaml` with `storeDir` / `cacheDir` because
    /// pnpm 11 reads those from the workspace YAML rather than `.npmrc`.
    pub fn add_mocked_registry(self) -> CommandTempCwd<AddMockedRegistry> {
        let store_dir = self.root.path().join("pacquet-store");
        let cache_dir = self.root.path().join("pacquet-cache");
        let npmrc_path = self.workspace.join(".npmrc");
        let npmrc_text = text_block_fnl! {
            "store-dir=../pacquet-store"
            "cache-dir=../pacquet-cache"
        };
        let mock_instance = AutoMockInstance::load_or_init();
        let mocked_registry = mock_instance.url();
        let npmrc_text = format!("registry={mocked_registry}\n{npmrc_text}");
        fs::write(&npmrc_path, npmrc_text).expect("write to .npmrc");

        let workspace_yaml = self.workspace.join("pnpm-workspace.yaml");
        let workspace_yaml_text = text_block_fnl! {
            "storeDir: ../pacquet-store"
            "cacheDir: ../pacquet-cache"
        };
        fs::write(&workspace_yaml, workspace_yaml_text).expect("write to pnpm-workspace.yaml");

        let npmrc_info = AddMockedRegistry { npmrc_path, store_dir, cache_dir, mock_instance };
        let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info: () } = self;
        CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info }
    }
}
