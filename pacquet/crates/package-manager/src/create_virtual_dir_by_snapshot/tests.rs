use super::{CreateVirtualDirBySnapshot, optimistic_wire_method};
use pacquet_config::PackageImportMethod;
use pacquet_lockfile::{PackageKey, SnapshotEntry};
use pacquet_reporter::{
    LogEvent, PackageImportMethod as WireImportMethod, ProgressMessage, Reporter,
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::tempdir;

pub(crate) struct LinkConcurrencyProbe {
    current: AtomicUsize,
    max: AtomicUsize,
    wait_for_overlap: bool,
    wait_started: AtomicBool,
    mutex: Mutex<()>,
    condvar: Condvar,
}

impl LinkConcurrencyProbe {
    pub(crate) fn waiting_for_overlap() -> Self {
        Self { wait_for_overlap: true, ..Self::default() }
    }

    pub(crate) fn max_concurrent(&self) -> usize {
        self.max.load(Ordering::SeqCst)
    }

    pub(super) fn enter(&self) -> LinkConcurrencyGuard<'_> {
        let current = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        let mut max = self.max.load(Ordering::SeqCst);
        while current > max {
            match self.max.compare_exchange_weak(max, current, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => {
                    self.condvar.notify_all();
                    break;
                }
                Err(next) => max = next,
            }
        }

        if self.wait_for_overlap && current == 1 && !self.wait_started.swap(true, Ordering::SeqCst)
        {
            let guard = self.mutex.lock().expect("lock link-concurrency probe");
            let _ = self
                .condvar
                .wait_timeout_while(guard, Duration::from_secs(2), |()| {
                    self.max.load(Ordering::SeqCst) < 2
                })
                .expect("wait for overlapping link");
        }

        LinkConcurrencyGuard { probe: self }
    }
}

impl Default for LinkConcurrencyProbe {
    fn default() -> Self {
        Self {
            current: AtomicUsize::new(0),
            max: AtomicUsize::new(0),
            wait_for_overlap: false,
            wait_started: AtomicBool::new(false),
            mutex: Mutex::new(()),
            condvar: Condvar::new(),
        }
    }
}

pub(super) struct LinkConcurrencyGuard<'a> {
    probe: &'a LinkConcurrencyProbe,
}

impl Drop for LinkConcurrencyGuard<'_> {
    fn drop(&mut self) {
        self.probe.current.fetch_sub(1, Ordering::SeqCst);
        self.probe.condvar.notify_all();
    }
}

/// `optimistic_wire_method` is the source of truth for the
/// configured-method → wire-method mapping the `imported` event
/// reports. `Auto` and `CloneOrCopy` collapse to `Clone` (the
/// optimistic first attempt); the explicit settings pass through.
/// A future change to pacquet's `PackageImportMethod` set must
/// either extend this match or fail this test.
#[test]
fn optimistic_wire_method_collapses_auto_and_clone_or_copy_to_clone() {
    assert_eq!(optimistic_wire_method(PackageImportMethod::Auto), WireImportMethod::Clone);
    assert_eq!(optimistic_wire_method(PackageImportMethod::CloneOrCopy), WireImportMethod::Clone);
    assert_eq!(optimistic_wire_method(PackageImportMethod::Clone), WireImportMethod::Clone);
    assert_eq!(optimistic_wire_method(PackageImportMethod::Hardlink), WireImportMethod::Hardlink);
    assert_eq!(optimistic_wire_method(PackageImportMethod::Copy), WireImportMethod::Copy);
}

/// `CreateVirtualDirBySnapshot::run` emits `pnpm:progress imported`
/// after `import_indexed_dir` succeeds. Driving with an empty
/// `cas_paths` map exercises the success path without hitting the
/// network: `import_indexed_dir` mkdirs the empty directory and
/// returns Ok, then the imported emit fires. Asserts the wire
/// fields (`method`, `requester`, `to`) match what the install
/// layer threaded down.
#[tokio::test]
async fn run_emits_imported_event_after_import_indexed_dir() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dir = tempdir().expect("tempdir");
    let virtual_store_dir = dir.path().to_path_buf();
    let cas_paths: HashMap<String, std::path::PathBuf> = HashMap::new();
    let logged_methods = AtomicU8::new(0);
    let snapshot = SnapshotEntry::default();
    let package_key: PackageKey = "react@18.0.0".parse().expect("valid v9 snapshot key");

    EVENTS.lock().unwrap().clear();

    // `tokio::task::block_in_place` matches how the production
    // call-site (the `warm_work` closure in `CreateVirtualStore`)
    // drives this from inside a multi-thread runtime; a
    // `current_thread` runtime would panic on `block_in_place`,
    // but `#[tokio::test]` defaults to single-thread, so we run
    // `.run()` directly here. The function itself is sync — only
    // the caller's runtime flavor matters.
    let layout = crate::VirtualStoreLayout::legacy(
        virtual_store_dir,
        pacquet_config::default_virtual_store_dir_max_length() as usize,
    );
    let skipped = crate::SkippedSnapshots::default();
    CreateVirtualDirBySnapshot {
        layout: &layout,
        cas_paths: &cas_paths,
        import_method: PackageImportMethod::Hardlink,
        logged_methods: &logged_methods,
        requester: "/proj",
        package_id: "react@18.0.0",
        package_key: &package_key,
        snapshot: &snapshot,
        skipped: &skipped,
        link_concurrency_probe: None,
    }
    .run::<RecordingReporter>()
    .expect("empty-cas-paths run should succeed");

    let captured = EVENTS.lock().unwrap();
    let imported = captured.iter().find_map(|event| match event {
        LogEvent::Progress(log) => match &log.message {
            ProgressMessage::Imported { method, requester, to } => {
                Some((*method, requester.clone(), to.clone()))
            }
            _ => None,
        },
        _ => None,
    });
    let (method, requester, to) =
        imported.unwrap_or_else(|| panic!("imported must fire; got {captured:?}"));
    assert_eq!(method, WireImportMethod::Hardlink);
    assert_eq!(requester, "/proj");
    // `to` is the per-package `node_modules/{name}` directory
    // inside the virtual store. The exact path depends on
    // `package_key.to_virtual_store_name()` and the temp dir
    // root, so spot-check the suffix via `Path::ends_with`
    // (component-based, so it works on Windows where `to` uses
    // backslashes too) instead of the full path.
    assert!(
        Path::new(&to).ends_with("react@18.0.0/node_modules/react"),
        "imported.to suffix must mirror the virtual-store layout; got {to}",
    );
}
