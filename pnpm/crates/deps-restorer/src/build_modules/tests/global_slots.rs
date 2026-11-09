use super::{
    super::{BuildModules, BuildModulesError, slots::virtual_store_dir_for_key},
    TEST_LOGGED_METHODS, gvs_layout, key, policy_from_specs, root_importers,
};
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pnpm_config::PackageImportMethod;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_patching::ExtendedPatchInfo;
use pnpm_reporter::SilentReporter;
use std::{collections::HashMap, fs, path::PathBuf};
use tempfile::{TempDir, tempdir};

/// The link phase stages a package this install builds. The build runs
/// there, and the built package is renamed into the slot.
#[test]
fn a_staged_package_is_built_and_renamed_into_its_slot() {
    let slot = PatchedSlot::new();
    slot.stage();

    slot.build().expect("build modules must complete cleanly");

    assert!(slot.pkg_dir.join("patched.txt").exists(), "the patched package must be placed");
    assert!(!slot.staging_dir().exists(), "the staging directory must be gone");
}

/// A slot that holds its package is final, so a build that another install
/// already placed is not repeated, and the staged copy is discarded.
#[test]
fn a_placed_package_is_left_alone() {
    let slot = PatchedSlot::new();
    slot.stage();
    fs::create_dir_all(&slot.pkg_dir).expect("create pkg dir");
    fs::write(slot.pkg_dir.join("package.json"), MANIFEST).expect("write manifest");

    slot.build().expect("build modules must complete cleanly");

    assert!(!slot.pkg_dir.join("patched.txt").exists(), "the placed package must not change");
    assert!(!slot.staging_dir().exists(), "the staging directory must be gone");
}

/// An import that did not finish leaves a package directory without its
/// `package.json`. The built package takes its place.
#[test]
fn a_built_package_replaces_an_unfinished_import() {
    let slot = PatchedSlot::new();
    slot.stage();
    fs::create_dir_all(&slot.pkg_dir).expect("create pkg dir");
    fs::write(slot.pkg_dir.join("partial.txt"), "").expect("write a partial import");

    slot.build().expect("build modules must complete cleanly");

    assert!(slot.pkg_dir.join("patched.txt").exists(), "the patched package must be placed");
    assert!(!slot.pkg_dir.join("partial.txt").exists(), "the unfinished import must be gone");
}

/// A build that fails leaves nothing in the slot, so no project links a
/// package whose build did not finish.
#[test]
fn a_failed_build_leaves_the_slot_without_its_package() {
    let slot = PatchedSlot::new();
    slot.stage();
    fs::write(slot.root.path().join("is-positive.patch"), "not a patch").expect("break the patch");

    slot.build().expect_err("a broken patch must fail the build");

    assert!(!slot.pkg_dir.exists(), "the slot must not hold the package");
    assert!(!slot.staging_dir().exists(), "the staging directory must be gone");
}

const MANIFEST: &str = r#"{"name":"is-positive","version":"1.0.0"}"#;

/// A patched `is-positive@1.0.0` that requires a build, in a
/// global-virtual-store slot, built with `--ignore-scripts`, so only the
/// patch writes to it.
struct PatchedSlot {
    root: TempDir,
    layout: &'static VirtualStoreLayout,
    pkg_key: PackageKey,
    pkg_dir: PathBuf,
}

impl PatchedSlot {
    fn new() -> Self {
        let root = tempdir().expect("create temp dir");
        let layout = gvs_layout(root.path());
        let pkg_key = key("is-positive", "1.0.0");
        let pkg_dir = virtual_store_dir_for_key(layout, &pkg_key);
        fs::write(
            root.path().join("is-positive.patch"),
            "\
diff --git a/patched.txt b/patched.txt
new file mode 100644
--- /dev/null
+++ b/patched.txt
@@ -0,0 +1 @@
+applied
",
        )
        .expect("write patch");
        Self { root, layout, pkg_key, pkg_dir }
    }

    fn staging_dir(&self) -> PathBuf {
        self.layout.staging_package_dir(&self.pkg_key)
    }

    /// Import the package into its staging directory, as the link phase
    /// does for a package this install builds.
    fn stage(&self) {
        let staging = self.staging_dir();
        fs::create_dir_all(&staging).expect("create staging dir");
        fs::write(staging.join("package.json"), MANIFEST).expect("write manifest");
    }

    fn build(&self) -> Result<crate::BuildModulesOutput, BuildModulesError> {
        let snapshots = HashMap::from([(self.pkg_key.clone(), SnapshotEntry::default())]);
        let requires_build = HashMap::from([(self.pkg_key.clone(), true)]);
        let importers = root_importers(&[("is-positive", "1.0.0")]);
        let policy = policy_from_specs([], true);
        let patches: HashMap<PackageKey, ExtendedPatchInfo> = HashMap::from([(
            self.pkg_key.without_peer(),
            ExtendedPatchInfo {
                hash: "0".repeat(64),
                patch_file_path: Some(self.root.path().join("is-positive.patch")),
                key: "is-positive@1.0.0".to_string(),
            },
        )]);
        let packages = HashMap::from([(
            self.pkg_key.without_peer(),
            super::empty_side_effects::registry_package(),
        )]);
        let modules_dir = self.root.path().join("node_modules");

        BuildModules {
            cache: crate::BuildCacheContext {
                captured: None,
                side_effects: crate::CachedSideEffects { overlays: None, rows: None },
                engine_name: None,
                read: false,
                write: false,
                publisher: None,
                store: None,
                frozen_store: false,
            },
            directories: crate::BuildLayout {
                link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
                layout: self.layout,
                pkg_roots_by_key: None,
                gather_ancestor_bin_paths: false,
                modules_dir: &modules_dir,
                lockfile_dir: self.root.path(),
                import_method: PackageImportMethod::Auto,
                logged_methods: &TEST_LOGGED_METHODS,
            },
            graph: crate::BuildGraphInputs {
                snapshots: Some(&snapshots),
                packages: Some(&packages),
                patches: Some(&patches),
                requires_build_by_snapshot: Some(&requires_build),
                importers: &importers,
                dependency_groups: None,
                build_scope: None,
                variant_platforms: None,
            },
            scripts: crate::BuildScriptOptions {
                extra_env: &HashMap::new(),
                user_agent: "pnpm/test",
                path: crate::ScriptPath { extra_bin_paths: &[], private_hoisting: false },
                shell: None,
                shell_emulator: false,
                unsafe_perm: true,
                ignore: true,
                patched_engines: crate::PatchedEngineCheck {
                    engine_strict: false,
                    node_version: None,
                    install_state_dir: None,
                },
            },
            allow_build_policy: &policy,
            child_concurrency: 1,
            skipped: &SkippedSnapshots::default(),
            rebuild: None,
        }
        .run::<SilentReporter>()
    }
}
