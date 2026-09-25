use super::{
    super::{BuildModules, slots::virtual_store_dir_for_key},
    TEST_LOGGED_METHODS, gvs_layout, key, policy_from_specs, root_importers,
};
use crate::SkippedSnapshots;
use pnpm_config::PackageImportMethod;
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_patching::ExtendedPatchInfo;
use pnpm_reporter::SilentReporter;
use std::{collections::HashMap, fs};
use tempfile::tempdir;

/// `--ignore-scripts` patches a package whose build scripts it leaves for a
/// later install in the same global-virtual-store slot, so the slot keeps
/// its `.pnpm-needs-build` marker. Removing it would make that install skip
/// the build, and a concurrent one mistake the slot for built.
#[test]
fn ignored_scripts_keep_the_marker_of_a_patched_slot() {
    let pkg_key = key("is-positive", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let requires_build = HashMap::from([(pkg_key.clone(), true)]);
    let importers = root_importers(&[("is-positive", "1.0.0")]);
    let policy = policy_from_specs([], true);
    let root = tempdir().expect("create temp dir");
    let layout = gvs_layout(root.path());
    let modules_dir = tempdir().expect("create modules dir");
    let lockfile_dir = tempdir().expect("create lockfile dir");

    let pkg_dir = virtual_store_dir_for_key(layout, &pkg_key);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    fs::write(pkg_dir.join("package.json"), r#"{"name":"is-positive","version":"1.0.0"}"#)
        .expect("write manifest");
    fs::write(pkg_dir.join(crate::NEEDS_BUILD_MARKER), "").expect("write marker");

    let patch_file = root.path().join("is-positive.patch");
    fs::write(
        &patch_file,
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
    let patches: HashMap<PackageKey, ExtendedPatchInfo> = HashMap::from([(
        pkg_key.without_peer(),
        ExtendedPatchInfo {
            hash: "0".repeat(64),
            patch_file_path: Some(patch_file),
            key: "is-positive@1.0.0".to_string(),
        },
    )]);

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
            layout,
            pkg_roots_by_key: None,
            gather_ancestor_bin_paths: false,
            modules_dir: modules_dir.path(),
            lockfile_dir: lockfile_dir.path(),
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
        },
        allow_build_policy: &policy,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        rebuild: None,
    }
    .run::<SilentReporter>()
    .expect("build modules must complete cleanly");

    assert!(pkg_dir.join("patched.txt").exists(), "the patch must still be applied");
    assert!(
        pkg_dir.join(crate::NEEDS_BUILD_MARKER).is_file(),
        "the withheld build must keep the slot's marker",
    );
}
