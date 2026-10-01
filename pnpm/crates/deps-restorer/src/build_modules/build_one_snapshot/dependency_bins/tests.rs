use super::{BuildModulesError, HashMap, HashSet, Mutex, PackageKey, Path, refresh_directory};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn shared_consumers_scan_once_until_another_dependency_completes() {
    let refreshed = Mutex::new(HashMap::new());
    let directory = Path::new("node_modules");
    let first: PackageKey = "first@1.0.0".parse().unwrap();
    let second: PackageKey = "second@1.0.0".parse().unwrap();
    let completed = HashSet::from([first.clone()]);
    let scans = AtomicUsize::new(0);
    let scan = || {
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
    assert!(!refreshed.lock().unwrap()[directory].contains(&second));

    refresh_directory(&refreshed, directory, &HashSet::from([second]), scan).unwrap();
    refresh_directory(&refreshed, directory, &completed, || {
        panic!("an already refreshed dependency must not trigger another scan");
    })
    .unwrap();
    assert_eq!(scans.load(Ordering::Relaxed), 2);
    assert!(refreshed.lock().unwrap()[directory].contains(&first));
}

#[test]
fn failed_scan_does_not_record_completed_dependencies() {
    let refreshed = Mutex::new(HashMap::new());
    let directory = Path::new("node_modules");
    let completed = HashSet::from(["first@1.0.0".parse().unwrap()]);
    let failed = refresh_directory(&refreshed, directory, &completed, || {
        Err(BuildModulesError::PatchFilePathMissing { dep_path: "test failure".into() })
    });
    assert!(failed.is_err());
    assert!(refreshed.lock().unwrap()[directory].is_empty());
    refresh_directory(&refreshed, directory, &completed, || Ok(())).unwrap();
    assert_eq!(refreshed.lock().unwrap()[directory], completed);
}
