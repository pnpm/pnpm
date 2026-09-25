use super::{
    super::{BuildModules, BuildModulesError, slots::virtual_store_dir_for_key},
    TEST_LOGGED_METHODS, gvs_layout, key, policy_from_specs, root_importers,
};
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pnpm_config::PackageImportMethod;
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_patching::ExtendedPatchInfo;
use pnpm_reporter::SilentReporter;
use std::{collections::HashMap, fs, path::PathBuf};
use tempfile::{TempDir, tempdir};

/// `--ignore-scripts` patches a package whose build scripts it leaves for a
/// later install in the same global-virtual-store slot, so the slot keeps
/// its `.pnpm-needs-build` marker. Removing it would make that install skip
/// the build, and a concurrent one mistake the slot for built.
#[test]
fn ignored_scripts_keep_the_marker_of_a_patched_slot() {
    let slot = PatchedSlot::new();
    fs::write(slot.pkg_dir.join(crate::NEEDS_BUILD_MARKER), "").expect("write marker");

    slot.build().expect("build modules must complete cleanly");

    assert!(slot.pkg_dir.join("patched.txt").exists(), "the patch must still be applied");
    assert!(
        slot.pkg_dir.join(crate::NEEDS_BUILD_MARKER).is_file(),
        "the withheld build must keep the slot's marker",
    );
}

/// A marker that exists but cannot be read leaves the slot's state unknown,
/// so the build fails instead of writing into a slot another install's
/// build may have left half-built.
#[test]
fn an_unreadable_marker_fails_the_build() {
    let slot = PatchedSlot::new();
    fs::create_dir(slot.pkg_dir.join(crate::NEEDS_BUILD_MARKER)).expect("make marker unreadable");

    let error = slot.build().expect_err("an unreadable marker must fail the build");

    assert!(matches!(error, BuildModulesError::ReadBuildMarker { .. }), "got {error:?}");
    assert!(!slot.pkg_dir.join("patched.txt").exists(), "the patch must not be applied");
}

/// A patched `is-positive@1.0.0` that requires a build, in a
/// global-virtual-store slot, built with `--ignore-scripts`.
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
        fs::create_dir_all(&pkg_dir).expect("create pkg dir");
        fs::write(pkg_dir.join("package.json"), r#"{"name":"is-positive","version":"1.0.0"}"#)
            .expect("write manifest");
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
        let modules_dir = self.root.path().join("node_modules");

        BuildModules {
            cache: crate::BuildCacheContext {
                maps_by_snapshot: None,
                engine_name: None,
                read: false,
                write: false,
                publisher: None,
                store_dir: None,
                store_index_writer: None,
                frozen_store: false,
            },
            directories: crate::BuildLayout {
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
                packages: None,
                patches: Some(&patches),
                requires_build_by_snapshot: Some(&requires_build),
                importers: &importers,
                dependency_groups: None,
            },
            scripts: crate::BuildScriptOptions {
                extra_env: &HashMap::new(),
                user_agent: "pnpm/test",
                prepend_node_path: ScriptsPrependNodePath::Never,
                shell: None,
                shell_emulator: false,
                unsafe_perm: true,
                ignore: true,
                patched_engines: crate::PatchedEngineCheck {
                    engine_strict: false,
                    node_version: None,
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
