use super::{
    BuildModulesError, DirectoryBinPlan, HashSet, HoistedBinPlans, PackageKey, Path,
    refresh_directory,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn shared_consumers_scan_once_until_another_dependency_completes() {
    let refreshed = HoistedBinPlans::default();
    let directory = Path::new("node_modules");
    refreshed
        .directory(directory)
        .lock()
        .unwrap()
        .plan = Some(DirectoryBinPlan::default());
    let first: PackageKey = "first@1.0.0".parse().unwrap();
    let second: PackageKey = "second@1.0.0".parse().unwrap();
    let completed = HashSet::from([first.clone()]);
    let scans = AtomicUsize::new(0);
    let scan = |_: &mut DirectoryBinPlan, _: &HashSet<PackageKey>| {
        scans.fetch_add(1, Ordering::Relaxed);
        Ok(())
    };
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                refresh_directory(&refreshed, directory, &completed, scan).unwrap();
            });
        }
    });
    assert_eq!(scans.load(Ordering::Relaxed), 1);
    assert!(
        !refreshed
            .directory(directory)
            .lock()
            .unwrap()
            .completed
            .contains(&second),
    );

    refresh_directory(
        &refreshed,
        directory,
        &HashSet::from([first.clone(), second.clone()]),
        |_, pending| {
            assert_eq!(pending, &HashSet::from([second]));
            scans.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    )
    .unwrap();
    refresh_directory(&refreshed, directory, &completed, |_, _| {
        panic!("an already refreshed dependency must not trigger another scan");
    })
    .unwrap();
    assert_eq!(scans.load(Ordering::Relaxed), 2);
    assert!(
        refreshed
            .directory(directory)
            .lock()
            .unwrap()
            .completed
            .contains(&first),
    );
}

#[test]
fn failed_scan_does_not_record_completed_dependencies() {
    let refreshed = HoistedBinPlans::default();
    let directory = Path::new("node_modules");
    refreshed
        .directory(directory)
        .lock()
        .unwrap()
        .plan = Some(DirectoryBinPlan::default());
    let completed = HashSet::from(["first@1.0.0".parse().unwrap()]);
    let failed = refresh_directory(&refreshed, directory, &completed, |_, _| {
        Err(BuildModulesError::PatchFilePathMissing { dep_path: "test failure".into() })
    });
    assert!(failed.is_err());
    assert!(
        refreshed
            .directory(directory)
            .lock()
            .unwrap()
            .completed
            .is_empty(),
    );
    refresh_directory(&refreshed, directory, &completed, |_, _| Ok(())).unwrap();
    assert_eq!(
        refreshed
            .directory(directory)
            .lock()
            .unwrap()
            .completed,
        completed,
    );
}

#[test]
fn separate_directories_refresh_concurrently() {
    let refreshed = HoistedBinPlans::default();
    let completed = HashSet::from(["first@1.0.0".parse().unwrap()]);
    let barrier = std::sync::Barrier::new(2);
    for directory in ["one/node_modules", "two/node_modules"] {
        refreshed
            .directory(Path::new(directory))
            .lock()
            .unwrap()
            .plan = Some(DirectoryBinPlan::default());
    }
    std::thread::scope(|scope| {
        for directory in ["one/node_modules", "two/node_modules"] {
            let completed = &completed;
            let refreshed = &refreshed;
            let barrier = &barrier;
            scope.spawn(move || {
                refresh_at_barrier(refreshed, Path::new(directory), completed, barrier);
            });
        }
    });
}

fn refresh_at_barrier(
    refreshed: &HoistedBinPlans,
    directory: &Path,
    completed: &HashSet<PackageKey>,
    barrier: &std::sync::Barrier,
) {
    refresh_directory(refreshed, directory, completed, |_, _| {
        barrier.wait();
        Ok(())
    })
    .unwrap();
}
