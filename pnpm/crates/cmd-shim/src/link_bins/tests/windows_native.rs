use crate::{
    capabilities::Host,
    link_bins::{LinkBinsOptions, PackageBinSource, link_bins_of_packages, remove_bin},
    shim::is_shim_pointing_at,
};
use pnpm_fs::test_support::with_retry_observer;
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
    let result = with_retry_observer(
        path,
        move |attempt| {
            sender.send(attempt.as_ref().copied().map_err(std::io::Error::raw_os_error)).unwrap();
            if attempt.is_err() {
                // Release only after a real failed attempt, not after a scheduled delay.
                drop(handle.take());
            }
        },
        operation,
    );
    let attempts: Vec<_> = receiver.try_iter().collect();
    eprintln!("operation result: {result:?}; real filesystem attempts: {attempts:?}");
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

/// Here the lock lands on the replacement rename, not on a removal.
#[test]
fn shim_replacement_recovers_after_transient_lock() {
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

    assert_recovers_after_lock(&shim, || {
        link_bins_of_packages::<Host>(&packages, &bins, &LinkBinsOptions::default())
    });

    let body = fs::read_to_string(&shim).unwrap();
    eprintln!("replacement shim:\n{body}");
    assert!(is_shim_pointing_at(&body, &target));
    assert_eq!(fs::read_to_string(target).unwrap(), content);
}

/// A Node.js entry is the one bin still deleted before it is replaced.
#[test]
fn node_binary_replacement_recovers_after_transient_lock() {
    let root = tempdir().unwrap();
    let package = root.path().join("node");
    fs::create_dir(&package).unwrap();
    let target = package.join("node.exe");
    let content = "new node binary";
    fs::write(&target, content).unwrap();
    let bins = root.path().join(".bin");
    fs::create_dir(&bins).unwrap();
    let shim = bins.join("node.exe");
    let old_target = root.path().join("old-node.exe");
    fs::write(&old_target, "old node binary").unwrap();
    fs::hard_link(&old_target, &shim).unwrap();
    let packages = [PackageBinSource::new(
        package,
        Arc::new(json!({"name": "node", "version": "1.0.0", "bin": "node.exe"})),
    )];
    let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();

    assert_recovers_after_lock(&shim, || {
        pool.install(|| {
            link_bins_of_packages::<Host>(&packages, &bins, &LinkBinsOptions::default())
        })
    });

    assert_eq!(fs::read_to_string(shim).unwrap(), content);
    assert_eq!(fs::read_to_string(old_target).unwrap(), "old node binary");
    assert_eq!(fs::read_to_string(target).unwrap(), content);
}
