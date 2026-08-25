use crate::{command_env::CommandTestExt, registry::TestRegistry};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use std::{fs, path::PathBuf, process::Command};
use tempfile::TempDir;
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
        // A prefix that names the owner: these trees hold a per-test store
        // and are the bulk of what an interrupted run abandons, and a sweep
        // can only safely delete what it can attribute (`.tmp*`, tempfile's
        // default, belongs to every crate that uses it).
        let root = tempfile::Builder::new()
            .prefix("pacquet-test-")
            .tempdir()
            .expect("create temporary directory");
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).expect("create temporary workspace for the commands");
        let pacquet = Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config();
        let pnpm = Command::new("pnpm").with_current_dir(&workspace).without_ambient_pnpm_config();
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
    /// Handle to the mocked registry this test uses — the process-scoped
    /// instance, or one of the test's own.
    pub mock_instance: TestRegistry,
}

impl AddMockedRegistry {
    /// Move `tag` to `version` on the mocked registry, the way the JS
    /// harness's `addDistTag` does — the setup behind every upstream test
    /// where a newer version is published after the install.
    ///
    /// The packument the last command cached goes with it, so the next one
    /// resolves against the moved tag. Whether a client notices a tag that
    /// moves under a packument it already holds is a caching question of
    /// its own, and not what any of these tests are about.
    pub fn set_dist_tag(&self, package: &str, version: &str, tag: &str) {
        self.mock_instance.set_dist_tag(package, version, tag);
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir).expect("drop the cached registry metadata");
        }
    }
}

impl CommandTempCwd<()> {
    /// Create a mock registry and a `.npmrc` file that defines `store-dir`, `cache-dir`, and `registry`.
    ///
    /// Also writes a `pnpm-workspace.yaml` with `storeDir` / `cacheDir` because
    /// pnpm 11 reads those from the workspace YAML rather than `.npmrc`.
    #[must_use]
    pub fn add_mocked_registry(self) -> CommandTempCwd<AddMockedRegistry> {
        self.add_mocked_registry_with_substitutions_and_mode(&[], false)
    }

    #[must_use]
    pub fn add_mocked_registry_with_pnpm_version(
        self,
        version: &str,
    ) -> CommandTempCwd<AddMockedRegistry> {
        self.add_mocked_registry_with_substitutions_and_mode(
            &[("0.0.0-test-current-pnpm", version)],
            true,
        )
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
        self.add_mocked_registry_with_substitutions_and_mode(substitutions, false)
    }

    /// Create a mock registry over fixture storage this test owns, so it may
    /// re-tag packages through [`AddMockedRegistry::set_dist_tag`] without
    /// any other test seeing the change.
    #[must_use]
    pub fn add_mocked_registry_with_own_storage(self) -> CommandTempCwd<AddMockedRegistry> {
        let mock_instance = TestRegistry::start_with_own_storage(self.root.path());
        self.write_registry_config(mock_instance)
    }

    fn add_mocked_registry_with_substitutions_and_mode(
        self,
        substitutions: &[(&str, &str)],
        static_registry: bool,
    ) -> CommandTempCwd<AddMockedRegistry> {
        let mock_instance = if substitutions.is_empty() {
            TestRegistry::start()
        } else {
            TestRegistry::start_over_built_storage(self.root.path(), substitutions, static_registry)
        };
        self.write_registry_config(mock_instance)
    }

    fn write_registry_config(
        self,
        mock_instance: TestRegistry,
    ) -> CommandTempCwd<AddMockedRegistry> {
        let store_dir = self.root.path().join("pacquet-store");
        let cache_dir = self.root.path().join("pacquet-cache");
        let npmrc_path = self.workspace.join(".npmrc");
        let npmrc_text = text_block_fnl! {
            "store-dir=../pacquet-store"
            "cache-dir=../pacquet-cache"
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
