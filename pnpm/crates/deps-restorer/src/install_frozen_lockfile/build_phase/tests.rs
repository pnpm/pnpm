use super::{
    BuildPhaseInputs, Config, HashMap, PackageKey, SkippedSnapshots,
    auto_installed_peer_bin_locations, resolve_snapshot_patches, run_build_phase,
};
use crate::{AllowBuildPolicy, VirtualStoreLayout};
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_lockfile::{
    GitResolution, LockfileResolution, PackageMetadata, ProjectSnapshot, SnapshotEntry,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_patching::{ExtendedPatchInfo, PatchGroup, PatchGroupRecord};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::StoreIndexWriter;
use std::sync::atomic::AtomicU8;
use tempfile::tempdir;

#[tokio::test]
async fn build_generated_peer_bin_is_considered_without_lockfile_has_bin() {
    let temp_dir = tempdir().expect("create temp dir");
    let layout = VirtualStoreLayout::legacy(temp_dir.path(), 120);
    let direct_key: PackageKey = "plugin@1.0.0(peer@1.0.0)".parse().expect("direct key");
    let peer_key: PackageKey = "peer@1.0.0".parse().expect("peer key");
    let importer: ProjectSnapshot = serde_json::from_value(serde_json::json!({
        "dependencies": { "plugin": { "specifier": "1.0.0", "version": "1.0.0(peer@1.0.0)" } }
    }))
    .expect("importer");
    let direct_metadata: PackageMetadata = serde_json::from_value(serde_json::json!({
        "resolution": { "integrity": "sha512-test" },
        "peerDependencies": { "peer": "*" }
    }))
    .expect("direct metadata");
    let peer_metadata: PackageMetadata = serde_json::from_value(serde_json::json!({
        "resolution": { "integrity": "sha512-test" }
    }))
    .expect("peer metadata");
    let snapshots = HashMap::from([(
        direct_key.clone(),
        serde_json::from_value(serde_json::json!({
            "dependencies": { "peer": "1.0.0" }
        }))
        .expect("direct snapshot"),
    )]);
    let packages = HashMap::from([
        (direct_key.without_peer(), direct_metadata),
        (peer_key.clone(), peer_metadata),
    ]);
    let requires_build = HashMap::from([(peer_key.clone(), true)]);
    let config = Config::new().leak();
    let (writer, writer_task) = StoreIndexWriter::spawn_disabled();
    let inputs = BuildPhaseInputs {
        cache: crate::BuildPhaseCache {
            maps_by_snapshot: &HashMap::new(),
            requires_build_by_snapshot: &requires_build,
            engine_name: None,
            store_index_writer: &writer,
        },
        directories: crate::BuildPhaseDirectories {
            workspace_root: temp_dir.path(),
            top_level_bin_root: temp_dir.path(),
            layout: &layout,
            hoisted_pkg_roots_by_key: None,
            is_hoisted: false,
            publicly_hoisted_for_post_build: &[],
            logged_methods: &AtomicU8::new(0),
            link_options: &LinkBinsOptions::default(),
        },
        graph: crate::BuildPhaseGraph {
            snapshots: Some(&snapshots),
            packages: Some(&packages),
            importers: &HashMap::new(),
            dependency_groups: &[DependencyGroup::Prod],
            materialized_snapshots: &[],
        },
        policy: crate::BuildPhasePolicy {
            config,
            patch_groups: None,
            allow_build_policy: &AllowBuildPolicy::default(),
            rebuild: None,
        },
        extra_env: &HashMap::new(),
        skipped: &SkippedSnapshots::default(),
        held_back_bins_dirs: &[],
    };
    assert_eq!(
        auto_installed_peer_bin_locations(&inputs, &importer),
        vec![layout.slot_dir(&peer_key).join("node_modules/peer"),]
    );
    drop(writer);
    writer_task.await.expect("join store writer").expect("drain store writer");
}

#[test]
fn resolves_git_snapshot_patch_from_package_version() {
    let patch = ExtendedPatchInfo {
        hash: "abc123".to_string(),
        patch_file_path: None,
        key: "foo@1.0.0".to_string(),
    };
    let mut group = PatchGroup::default();
    group.exact.insert("1.0.0".to_string(), patch.clone());
    let groups = PatchGroupRecord::from([("foo".to_string(), group)]);
    let key = "foo@git+file:///repo#0123456789012345678901234567890123456789(patch_hash=abc123)"
        .parse::<PackageKey>()
        .expect("parse git snapshot key");
    let snapshots = HashMap::from([(key.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([(
        key.without_peer(),
        PackageMetadata {
            resolution: LockfileResolution::Git(GitResolution {
                repo: "file:///repo".to_string(),
                commit: "0123456789012345678901234567890123456789".to_string(),
                integrity: None,
                path: None,
            }),
            version: Some("1.0.0".to_string()),
            engines: None,
            cpu: None,
            os: None,
            libc: None,
            deprecated: None,
            has_bin: None,
            prepare: None,
            bundled_dependencies: None,
            peer_dependencies: None,
            peer_dependencies_meta: None,
        },
    )]);

    let patches =
        resolve_snapshot_patches(&Config::new(), Some(&groups), Some(&snapshots), Some(&packages))
            .expect("resolve snapshot patches")
            .expect("patches are configured");

    assert_eq!(patches.get(&key.without_peer()), Some(&patch));
}

#[tokio::test]
async fn ignored_scripts_fast_path_defers_only_materialized_snapshots() {
    let materialized = "materialized@1.0.0".parse::<PackageKey>().expect("parse package key");
    let unrelated = "unrelated@1.0.0".parse::<PackageKey>().expect("parse package key");
    let requires_build_by_snapshot =
        HashMap::from([(materialized.clone(), true), (unrelated, true)]);
    let materialized_snapshots = [materialized.clone()];

    let temp_dir = tempdir().expect("create temp dir");
    let mut config = Config::new();
    config.ignore_scripts = true;
    config.virtual_store_only = true;
    let config = config.leak();
    let layout = VirtualStoreLayout::legacy(temp_dir.path(), 120);
    let importers = HashMap::<String, ProjectSnapshot>::new();
    let dependency_groups = Vec::<DependencyGroup>::new();
    let side_effects_maps_by_snapshot = HashMap::new();
    let allow_build_policy = AllowBuildPolicy::default();
    let extra_env = HashMap::new();
    let skipped = SkippedSnapshots::default();
    let logged_methods = AtomicU8::new(0);
    let (store_index_writer, writer_task) = StoreIndexWriter::spawn_disabled();

    let output = run_build_phase::<SilentReporter>(&BuildPhaseInputs {
        cache: crate::BuildPhaseCache {
            maps_by_snapshot: &side_effects_maps_by_snapshot,
            requires_build_by_snapshot: &requires_build_by_snapshot,
            engine_name: None,
            store_index_writer: &store_index_writer,
        },
        directories: crate::BuildPhaseDirectories {
            workspace_root: temp_dir.path(),
            top_level_bin_root: temp_dir.path(),
            layout: &layout,
            hoisted_pkg_roots_by_key: None,
            is_hoisted: false,
            publicly_hoisted_for_post_build: &[],
            logged_methods: &logged_methods,
            link_options: &LinkBinsOptions::default(),
        },
        graph: crate::BuildPhaseGraph {
            snapshots: None,
            packages: None,
            importers: &importers,
            dependency_groups: &dependency_groups,
            materialized_snapshots: &materialized_snapshots,
        },
        policy: crate::BuildPhasePolicy {
            config,
            patch_groups: None,
            allow_build_policy: &allow_build_policy,
            rebuild: None,
        },

        extra_env: &extra_env,

        skipped: &skipped,
        held_back_bins_dirs: &[],
    })
    .expect("build phase succeeds");

    assert_eq!(output.deferred_builds, [materialized.to_string()]);
    drop(store_index_writer);
    writer_task.await.expect("join writer task").expect("drain writer task");
}
