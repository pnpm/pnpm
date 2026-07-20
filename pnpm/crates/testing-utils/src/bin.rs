use crate::registry::TestRegistry;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
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
    #[must_use]
    pub fn init() -> Self {
        let root = tempdir().expect("create temporary directory");
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).expect("create temporary workspace for the commands");
        let pacquet =
            Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(&workspace);
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
    /// Handle to the process-scoped mocked registry used by this test.
    pub mock_instance: TestRegistry,
}

impl CommandTempCwd<()> {
    /// Create a mock registry and a `.npmrc` file that defines `store-dir`, `cache-dir`, and `registry`.
    ///
    /// Also writes a `pnpm-workspace.yaml` with `storeDir` / `cacheDir` because
    /// pnpm 11 reads those from the workspace YAML rather than `.npmrc`.
    #[must_use]
    pub fn add_mocked_registry(self) -> CommandTempCwd<AddMockedRegistry> {
        self.add_mocked_registry_with_substitutions(&[])
    }

    /// Create a mock registry whose generated fixture manifests have exact
    /// strings replaced for this test run. The storage lives under the
    /// command fixture's temp root, so `git+file://` replacements remain
    /// valid for the registry's lifetime.
    #[must_use]
    pub fn add_mocked_registry_with_substitutions(
        self,
        substitutions: &[(&str, &str)],
    ) -> CommandTempCwd<AddMockedRegistry> {
        let store_dir = self.root.path().join("pacquet-store");
        let cache_dir = self.root.path().join("pacquet-cache");
        let npmrc_path = self.workspace.join(".npmrc");
        let npmrc_text = text_block_fnl! {
            "store-dir=../pacquet-store"
            "cache-dir=../pacquet-cache"
        };
        let mock_instance = if substitutions.is_empty() {
            TestRegistry::start()
        } else {
            let registry_storage = self.root.path().join("registry-storage");
            pnpr_fixtures::build_storage_at_with_substitutions(
                &pnpr_fixtures::packages_dir(),
                &registry_storage,
                substitutions,
            );
            TestRegistry::start_with_storage(&registry_storage)
        };
        let mocked_registry = mock_instance.url();
        let npmrc_text = format!("registry={mocked_registry}\n{npmrc_text}");
        fs::write(&npmrc_path, npmrc_text).expect("write to .npmrc");

        // Explicitly pin `enableGlobalVirtualStore: false` so a test
        // is hermetic regardless of any GVS opt-in the developer
        // has set in their global pnpm config (`~/.config/pnpm/config.yaml`
        // on Linux/macOS-with-XDG, `~/Library/Preferences/pnpm/config.yaml`
        // on macOS by default). Tests that exercise GVS explicitly
        // override this — see `enable_gvs_in_workspace_yaml` in
        // `pnpm/crates/cli/tests/_utils.rs`.
        let workspace_yaml = self.workspace.join("pnpm-workspace.yaml");
        let workspace_yaml_text = text_block_fnl! {
            "storeDir: ../pacquet-store"
            "cacheDir: ../pacquet-cache"
            "enableGlobalVirtualStore: false"
        };
        fs::write(&workspace_yaml, workspace_yaml_text).expect("write to pnpm-workspace.yaml");

        let npmrc_info = AddMockedRegistry { npmrc_path, store_dir, cache_dir, mock_instance };
        let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info: () } = self;
        CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info }
    }
}
