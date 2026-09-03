use super::{
    SnapshotPlan, SnapshotPlanInputs, optional_children_match, optional_children_match_with,
    plan_snapshots,
};
use crate::{
    AllowBuildPolicy, CreateVirtualStoreError, SkippedSnapshots, VirtualStoreLayout,
    create_virtual_store::SnapshotCacheKey,
};
use pnpm_lockfile::{
    DirectoryResolution, LockfileResolution, PackageKey, PackageMetadata, PkgName,
    RegistryResolution, SnapshotDepRef, SnapshotEntry,
};
use pnpm_reporter::SilentReporter;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::Path,
};

#[test]
fn optional_child_probe_propagates_io_errors() {
    let snapshot_key: PackageKey = "parent@1.0.0".parse().expect("parse snapshot key");
    let snapshot = SnapshotEntry {
        optional_dependencies: Some(HashMap::from([(
            PkgName::parse("optional-child").expect("parse alias"),
            SnapshotDepRef::Plain("1.0.0".parse().expect("parse version")),
        )])),
        ..SnapshotEntry::default()
    };
    let layout = VirtualStoreLayout::legacy("virtual-store", 120);
    let error = optional_children_match_with(
        &snapshot_key,
        &snapshot,
        &layout,
        &SkippedSnapshots::default(),
        true,
        true,
        |_, _| Err(io::Error::new(io::ErrorKind::PermissionDenied, "fixture denial")),
    )
    .expect_err("permission errors must abort the warm-slot probe");

    assert!(matches!(
        error,
        CreateVirtualStoreError::InspectOptionalDependency { error, .. }
            if error.kind() == io::ErrorKind::PermissionDenied
    ));
}

#[test]
fn invalid_optional_child_entries_do_not_match() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let snapshot_key: PackageKey = "parent@1.0.0".parse().expect("parse snapshot key");
    let snapshot = SnapshotEntry {
        optional_dependencies: Some(HashMap::from([(
            PkgName::parse("optional-child").expect("parse alias"),
            SnapshotDepRef::Plain("1.0.0".parse().expect("parse version")),
        )])),
        ..SnapshotEntry::default()
    };
    let layout = VirtualStoreLayout::legacy(temp_dir.path().join("virtual-store"), 120);
    let child_path = layout.slot_dir(&snapshot_key).join("node_modules").join("optional-child");
    fs::create_dir_all(child_path.parent().expect("child path has a parent"))
        .expect("create slot modules directory");
    let target = temp_dir.path().join("optional-target");
    fs::create_dir(&target).expect("create optional target");
    pnpm_fs::symlink_dir(&target, &child_path).expect("link optional child");

    assert!(
        optional_children_match(
            &snapshot_key,
            &snapshot,
            &layout,
            &SkippedSnapshots::default(),
            true,
            true,
        )
        .expect("inspect valid optional child"),
    );

    fs::remove_dir(&target).expect("remove optional target");
    assert!(
        !optional_children_match(
            &snapshot_key,
            &snapshot,
            &layout,
            &SkippedSnapshots::default(),
            true,
            true,
        )
        .expect("inspect dangling optional child"),
    );
    assert!(
        !optional_children_match(
            &snapshot_key,
            &snapshot,
            &layout,
            &SkippedSnapshots::default(),
            true,
            false,
        )
        .expect("inspect unexpected dangling optional child"),
    );

    pnpm_fs::remove_symlink_dir(&child_path).expect("remove dangling optional child");
    fs::write(&child_path, "not a directory").expect("create invalid optional child");
    assert!(
        !optional_children_match(
            &snapshot_key,
            &snapshot,
            &layout,
            &SkippedSnapshots::default(),
            true,
            true,
        )
        .expect("inspect non-directory optional child"),
    );
}

const DUMMY_SHA512: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn registry_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: DUMMY_SHA512.parse().expect("parse integrity"),
            revision: None,
        }),
        version: None,
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
    }
}

struct PlanFixture {
    snapshots: HashMap<PackageKey, SnapshotEntry>,
    packages: HashMap<PackageKey, PackageMetadata>,
    layout: VirtualStoreLayout,
}

impl PlanFixture {
    fn gvs(store_root: &Path, metadata: PackageMetadata) -> Self {
        let snapshot_key: PackageKey = "foo@1.0.0".parse().expect("parse snapshot key");
        let snapshots = HashMap::from([(snapshot_key.clone(), SnapshotEntry::default())]);
        let packages = HashMap::from([(snapshot_key, metadata)]);
        let layout = VirtualStoreLayout::global(
            store_root.join("links"),
            120,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            None,
        );
        PlanFixture { snapshots, packages, layout }
    }

    fn snapshot_key(&self) -> &PackageKey {
        self.snapshots.keys().next().expect("fixture holds one snapshot")
    }

    fn slot_package_dir(&self) -> std::path::PathBuf {
        self.layout
            .slot_dir(self.snapshot_key())
            .join("node_modules")
            .join(self.snapshot_key().name.to_string())
    }

    fn materialize_slot(&self) {
        let dir = self.slot_package_dir();
        fs::create_dir_all(&dir).expect("materialize the slot's package dir");
        // The import places the completion marker (`package.json`) last;
        // a slot without it is a partial import.
        fs::write(dir.join("package.json"), "{}").expect("place the completion marker");
    }

    fn plan(&self, force: bool) -> SnapshotPlan<'_> {
        self.plan_inner(false, force)
    }

    /// Plan with a current lockfile that matches the wanted one, as a
    /// completed previous install would have recorded it.
    fn plan_with_matching_current(&self, force: bool) -> SnapshotPlan<'_> {
        self.plan_inner(true, force)
    }

    fn plan_inner(&self, current_matches_wanted: bool, force: bool) -> SnapshotPlan<'_> {
        let allow_build_policy = AllowBuildPolicy::new(HashSet::new(), HashSet::new(), false);
        let mut cache_keys = self
            .snapshots
            .keys()
            .map(|snapshot_key| {
                (
                    snapshot_key.clone(),
                    Ok(SnapshotCacheKey {
                        value: Some(format!("cache-key:{snapshot_key}")),
                        is_git_hosted: false,
                    }),
                )
            })
            .collect();
        plan_snapshots::<SilentReporter>(SnapshotPlanInputs {
            snapshots: &self.snapshots,
            packages: &self.packages,
            current_snapshots: current_matches_wanted.then_some(&self.snapshots),
            current_packages: current_matches_wanted.then_some(&self.packages),
            layout: &self.layout,
            allow_build_policy: &allow_build_policy,
            skipped: &SkippedSnapshots::default(),
            link_dependencies: true,
            force,
            is_hoisted: false,
            include_optional_dependencies: true,
            cache_keys: &mut cache_keys,
        })
        .expect("plan snapshots")
    }
}

#[test]
fn gvs_existing_slot_is_skipped_without_a_current_lockfile() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let fixture = PlanFixture::gvs(temp_dir.path(), registry_metadata());
    fixture.materialize_slot();

    let plan = fixture.plan(false);

    assert!(
        plan.survivors.is_empty(),
        "an existing content-addressed slot needs no re-materialization",
    );
    assert_eq!(plan.skipped_entries.len(), 1);
    let (snapshot_key, _, cache_key) = &plan.skipped_entries[0];
    assert_eq!(*snapshot_key, fixture.snapshot_key());
    assert!(
        cache_key.is_some(),
        "the skipped entry must keep its cache key so its store-index rows still feed the build phase",
    );
}

#[test]
fn gvs_partial_slot_without_completion_marker_survives() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let fixture = PlanFixture::gvs(temp_dir.path(), registry_metadata());
    fs::create_dir_all(fixture.slot_package_dir())
        .expect("materialize the slot's package dir without its completion marker");

    let plan = fixture.plan(false);

    assert_eq!(
        plan.survivors.len(),
        1,
        "a slot directory without its completion marker is a partial import and must be repaired",
    );
}

#[test]
fn gvs_slot_missing_a_regular_child_link_survives() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let parent_key: PackageKey = "foo@1.0.0".parse().expect("parse snapshot key");
    let child_key: PackageKey = "bar@1.0.0".parse().expect("parse snapshot key");
    let parent_snapshot = SnapshotEntry {
        dependencies: Some(HashMap::from([(
            PkgName::parse("bar").expect("parse alias"),
            SnapshotDepRef::Plain("1.0.0".parse().expect("parse version")),
        )])),
        ..SnapshotEntry::default()
    };
    let snapshots = HashMap::from([
        (parent_key.clone(), parent_snapshot),
        (child_key.clone(), SnapshotEntry::default()),
    ]);
    let packages = HashMap::from([
        (parent_key.clone(), registry_metadata()),
        (child_key.clone(), registry_metadata()),
    ]);
    let layout = VirtualStoreLayout::global(
        temp_dir.path().join("links"),
        120,
        None,
        Some(&snapshots),
        Some(&packages),
        None,
        None,
    );
    let fixture = PlanFixture { snapshots, packages, layout };
    let parent_dir = fixture.layout.slot_dir(&parent_key).join("node_modules").join("foo");
    fs::create_dir_all(&parent_dir).expect("materialize the parent slot");
    fs::write(parent_dir.join("package.json"), "{}").expect("place the completion marker");

    let plan = fixture.plan(false);
    let survivor_keys: HashSet<String> =
        plan.survivors.iter().map(|(key, _, _)| key.to_string()).collect();
    assert!(
        survivor_keys.contains("foo@1.0.0"),
        "a marker-complete slot missing a child link is a partial import and must be repaired",
    );

    let child_link = parent_dir.parent().expect("modules dir").join("bar");
    fs::create_dir(&child_link).expect("plant a plain directory where the child link belongs");
    let plan = fixture.plan(false);
    let survivor_keys: HashSet<String> =
        plan.survivors.iter().map(|(key, _, _)| key.to_string()).collect();
    assert!(
        survivor_keys.contains("foo@1.0.0"),
        "a plain directory where the child link belongs is a corrupted slot and must be repaired",
    );
    fs::remove_dir(&child_link).expect("remove the plain directory");

    let child_dir = fixture.layout.slot_dir(&child_key).join("node_modules").join("bar");
    fs::create_dir_all(&child_dir).expect("materialize the child slot");
    fs::write(child_dir.join("package.json"), "{}").expect("place the child completion marker");
    pnpm_fs::symlink_dir(&child_dir, &child_link).expect("link the child into the parent slot");

    let plan = fixture.plan(false);
    assert!(
        plan.survivors.is_empty(),
        "with the marker and every child link present both slots are complete",
    );
}

#[test]
fn gvs_missing_slot_survives_without_a_current_lockfile() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let fixture = PlanFixture::gvs(temp_dir.path(), registry_metadata());

    let plan = fixture.plan(false);

    assert_eq!(plan.survivors.len(), 1, "a missing slot must be materialized");
    assert!(plan.skipped_entries.is_empty());
}

#[test]
fn force_defeats_the_gvs_existing_slot_skip() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let fixture = PlanFixture::gvs(temp_dir.path(), registry_metadata());
    fixture.materialize_slot();

    let plan = fixture.plan(true);

    assert_eq!(plan.survivors.len(), 1, "--force must re-materialize an existing slot");
}

#[test]
fn force_defeats_the_current_lockfile_skip() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let gvs_fixture = PlanFixture::gvs(temp_dir.path(), registry_metadata());
    let fixture = PlanFixture {
        layout: VirtualStoreLayout::legacy(temp_dir.path().join("virtual-store"), 120),
        ..gvs_fixture
    };
    fixture.materialize_slot();

    let plan = fixture.plan_with_matching_current(true);

    assert_eq!(
        plan.survivors.len(),
        1,
        "--force must re-materialize even a slot the current lockfile vouches for",
    );
}

#[test]
fn gvs_directory_resolution_is_never_skipped_by_slot_existence() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let metadata = PackageMetadata {
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: "local-foo".to_string(),
        }),
        ..registry_metadata()
    };
    let fixture = PlanFixture::gvs(temp_dir.path(), metadata);
    fixture.materialize_slot();

    let plan = fixture.plan(false);

    assert_eq!(
        plan.survivors.len(),
        1,
        "a directory resolution's source is mutable, so its slot must be re-imported",
    );
}

#[test]
fn existing_slot_without_gvs_still_survives_without_a_current_lockfile() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let gvs_fixture = PlanFixture::gvs(temp_dir.path(), registry_metadata());
    let fixture = PlanFixture {
        layout: VirtualStoreLayout::legacy(temp_dir.path().join("virtual-store"), 120),
        ..gvs_fixture
    };
    fixture.materialize_slot();

    let plan = fixture.plan(false);

    assert_eq!(
        plan.survivors.len(),
        1,
        "a project-local slot is not content-addressed, so existence alone must not skip it",
    );
}
