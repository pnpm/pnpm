use super::{super::import_indexed_dir, FORCE_SHARED, cas_map, write_source};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{fs, sync::atomic::AtomicU8};
use tempfile::tempdir;

#[test]
fn safe_to_skip_keeps_a_target_a_concurrent_importer_already_completed() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"{\"version\":\"1.0.0\"}").unwrap();
    fs::write(target.join("index.js"), b"module.exports = 1").unwrap();

    let logged_methods = AtomicU8::new(0);
    import_indexed_dir::<SilentReporter>(
        &logged_methods,
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("a slot another importer already completed is not a conflict");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
    assert_eq!(
        logged_methods.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "an already matching shared slot must not be imported into a throwaway stage",
    );
    let strays: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "cas" && name != "slot")
        .collect();
    assert_eq!(strays, Vec::<String>::new(), "staging dir must be cleaned up");
}
#[test]
fn concurrent_importers_of_one_shared_slot_both_succeed() {
    use std::sync::{Arc, Barrier};

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = Arc::new(cas_map(&[("package.json", pkg_json), ("index.js", index)]));

    let target = Arc::new(tmp.path().join("slot"));
    let barrier = Arc::new(Barrier::new(2));

    let handles: Vec<_> = (0..2)
        .map(|_| {
            let cas = Arc::clone(&cas);
            let target = Arc::clone(&target);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                import_indexed_dir::<SilentReporter>(
                    &AtomicU8::new(0),
                    PackageImportMethod::Copy,
                    &target,
                    &cas,
                    FORCE_SHARED,
                )
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("importer thread panicked").expect("both importers must succeed");
    }

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
    let strays: Vec<String> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "cas" && name != "slot")
        .collect();
    assert_eq!(strays, Vec::<String>::new(), "neither importer may leak a staging dir");
}
