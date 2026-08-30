use super::{
    AllowBuildPolicy, AtomicU8, BuildPhaseInputs, Config, DependencyGroup, HashMap, PackageKey,
    ProjectSnapshot, SkippedSnapshots, StoreIndexWriter, VirtualStoreLayout,
    resolve_snapshot_patches, run_build_phase,
};
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_lockfile::{GitResolution, LockfileResolution, PackageMetadata, SnapshotEntry};
use pnpm_patching::{ExtendedPatchInfo, PatchGroup, PatchGroupRecord};
use pnpm_reporter::SilentReporter;
use tempfile::tempdir;

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
        config,
        workspace_root: temp_dir.path(),
        top_level_bin_root: temp_dir.path(),
        layout: &layout,
        snapshots: None,
        packages: None,
        importers: &importers,
        dependency_groups: &dependency_groups,
        patch_groups: None,
        allow_build_policy: &allow_build_policy,
        side_effects_maps_by_snapshot: &side_effects_maps_by_snapshot,
        requires_build_by_snapshot: &requires_build_by_snapshot,
        materialized_snapshots: &materialized_snapshots,
        engine_name: None,
        extra_env: &extra_env,
        store_index_writer: &store_index_writer,
        skipped: &skipped,
        hoisted_pkg_roots_by_key: None,
        is_hoisted: false,
        publicly_hoisted_for_post_build: &[],
        logged_methods: &logged_methods,
        rebuild: None,
        link_options: &LinkBinsOptions::default(),
    })
    .expect("build phase succeeds");

    assert_eq!(output.deferred_builds, [materialized.to_string()]);
    drop(store_index_writer);
    writer_task.await.expect("join writer task").expect("drain writer task");
}
