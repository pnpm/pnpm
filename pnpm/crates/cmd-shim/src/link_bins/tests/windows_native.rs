use crate::{
    capabilities::Host,
    link_bins::{LinkBinsOptions, PackageBinSource, link_bins_of_packages, remove_bin},
    shim::is_shim_pointing_at,
};
use pnpm_fs::test_support::with_file_removal_observer;
use serde_json::json;
use std::{
    fmt::Debug,
    fs,
    os::windows::fs::OpenOptionsExt,
    path::Path,
    sync::{Arc, mpsc},
};
use tempfile::tempdir;

fn assert_recovers_after_lock<Error: Debug>(
    path: &Path,
    operation: impl FnOnce() -> Result<(), Error>,
) {
    // Permit normal reads and writes, but omit FILE_SHARE_DELETE.
    let mut handle =
        Some(fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2).open(path).unwrap());
    let (sender, receiver) = mpsc::channel();
    let result = with_file_removal_observer(
        path,
        move |attempt| {
            sender.send(attempt.as_ref().copied().map_err(std::io::Error::raw_os_error)).unwrap();
            if attempt.is_err() {
                // Release only after a real failed deletion, not after a scheduled delay.
                drop(handle.take());
            }
        },
        operation,
    );
    let attempts: Vec<_> = receiver.try_iter().collect();
    eprintln!("removal result: {result:?}; real filesystem attempts: {attempts:?}");
    result.expect("the operation must recover after the deny-delete handle closes");
    assert!(matches!(attempts.first(), Some(Err(Some(5 | 32 | 33)))));
    assert_eq!(attempts.last(), Some(&Ok(())));
}

#[test]
fn cleanup_recovers_after_transient_lock() {
    let root = tempdir().unwrap();
    let target = root.path().join("program");
    fs::write(&target, "program content").unwrap();
    let bins = root.path().join(".bin");
    fs::create_dir(&bins).unwrap();
    let shim = bins.join("foo");
    fs::hard_link(&target, &shim).unwrap();
    for suffix in ["cmd", "ps1", "exe"] {
        fs::write(bins.join(format!("foo.{suffix}")), "old shim").unwrap();
    }

    assert_recovers_after_lock(&shim, || remove_bin(&shim));

    assert_eq!(fs::read_dir(&bins).unwrap().count(), 0);
    assert_eq!(fs::read_to_string(target).unwrap(), "program content");
}

#[test]
fn replacement_recovers_after_transient_lock() {
    let root = tempdir().unwrap();
    let package = root.path().join("foo");
    fs::create_dir(&package).unwrap();
    let target = package.join("cli.js");
    let content = "#!/usr/bin/env node\nconsole.log('new target')\n";
    fs::write(&target, content).unwrap();
    let bins = root.path().join(".bin");
    fs::create_dir(&bins).unwrap();
    let shim = bins.join("foo");
    fs::write(&shim, "stale shim pointing at an obsolete program").unwrap();
    let packages = [PackageBinSource::new(
        package,
        Arc::new(json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"})),
    )];
    let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();

    assert_recovers_after_lock(&shim, || {
        pool.install(|| {
            link_bins_of_packages::<Host>(&packages, &bins, &LinkBinsOptions::default())
        })
    });

    let body = fs::read_to_string(&shim).unwrap();
    eprintln!("replacement shim:\n{body}");
    assert!(is_shim_pointing_at(&body, &target));
    for suffix in ["cmd", "ps1"] {
        let body = fs::read_to_string(bins.join(format!("foo.{suffix}"))).unwrap();
        assert!(!body.is_empty(), "missing Windows shim body for {suffix}");
    }
    assert_eq!(fs::read_to_string(target).unwrap(), content);
}
