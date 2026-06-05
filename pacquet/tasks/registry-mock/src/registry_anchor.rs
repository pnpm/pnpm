use crate::{MockInstanceOptions, RegistryInfo, process_kill::kill_process_by_pid};
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};
use std::{
    env::temp_dir,
    fs::{self, File, OpenOptions, TryLockError},
    mem::forget,
    path::{Path, PathBuf},
    sync::OnceLock,
};
use sysinfo::{Pid, Signal};

/// Count references and automatically manage a single shared mocked registry server instance that is spawned
/// by the first test to run.
///
/// The reference counter increases on [load](RegistryAnchor::load_or_init) and decreases on [drop](Drop).
#[derive(Debug, Serialize, Deserialize)]
pub struct RegistryAnchor {
    pub ref_count: u32,
    pub info: RegistryInfo,
}

impl Drop for RegistryAnchor {
    fn drop(&mut self) {
        // information from self is outdated, do not use it.

        let guard = GuardFile::lock();

        // load an up-to-date anchor, it is leaked to prevent dropping (again).
        let anchor = RegistryAnchor::load().pipe(Box::new).pipe(Box::leak);
        if self.info != anchor.info {
            eprintln!("info: {:?} is outdated. Skip.", &self.info);
            return;
        }

        if let Some(ref_count) = anchor.ref_count.checked_sub(1) {
            anchor.ref_count = ref_count;
            anchor.save();
            if ref_count > 0 {
                eprintln!("info: The mocked server is still used by {ref_count} users. Skip.");
                return;
            }
        }

        let pid = anchor.info.pid;
        eprintln!("info: There are no more users that use the mocked server");
        eprintln!("info: Terminating pnpr pid {pid}...");
        let killed = kill_process_by_pid(Pid::from_u32(pid), Signal::Interrupt);
        eprintln!("info: kill signal delivered: {killed}");

        RegistryAnchor::delete();
        guard.unlock();
    }
}

impl RegistryAnchor {
    fn path() -> &'static Path {
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| temp_dir().join("pacquet-registry-mock-anchor.json"))
    }

    fn load() -> Self {
        RegistryAnchor::path()
            .pipe(fs::read_to_string)
            .expect("read the anchor")
            .pipe_as_ref(serde_json::from_str)
            .expect("parse anchor")
    }

    fn save(&self) {
        let text = serde_json::to_string_pretty(self).expect("convert anchor to JSON");
        fs::write(RegistryAnchor::path(), text).expect("write to anchor");
    }

    #[must_use]
    pub fn load_or_init(init_options: MockInstanceOptions<'_>) -> Self {
        if let Some(guard) = GuardFile::try_lock() {
            // Run the spawn on a fresh OS thread so the freshly built tokio
            // runtime is not nested inside a caller's runtime. Tokio panics
            // with `Cannot start a runtime from within a runtime` whenever
            // `block_on` runs on a thread that already drives a runtime,
            // regardless of flavor, and `#[tokio::test]` callers reach this
            // function from such a thread.
            let mock_instance = std::thread::scope(|scope| {
                scope
                    .spawn(move || {
                        tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("build tokio runtime")
                            .block_on(init_options.spawn())
                    })
                    .join()
                    .expect("registry spawn thread panicked")
            });
            let port = init_options.port;
            let pid = mock_instance.process.id();
            let info = RegistryInfo { port, pid };
            let anchor = RegistryAnchor { ref_count: 1, info };
            anchor.save();
            guard.unlock();
            forget(mock_instance); // prevent this process from killing itself on drop
            anchor
        } else {
            let guard = GuardFile::lock();
            let mut anchor = RegistryAnchor::load();
            anchor.ref_count = anchor.ref_count.checked_add(1).expect("increment ref_count");
            anchor.save();
            guard.unlock();
            anchor
        }
    }

    fn delete() {
        if let Err(error) = fs::remove_file(RegistryAnchor::path()) {
            eprintln!("warn: Failed to delete the anchor file: {error}");
        }
    }
}

/// Prevent race condition between multiple tests.
#[must_use]
struct GuardFile;

impl Drop for GuardFile {
    fn drop(&mut self) {
        GuardFile::path().unlock().expect("release file guard");
    }
}

impl GuardFile {
    fn path() -> &'static File {
        static PATH: OnceLock<File> = OnceLock::new();
        PATH.get_or_init(|| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(temp_dir().join("pacquet-registry-mock-anchor.lock"))
                .expect("open the guard file")
        })
    }

    fn lock() -> Self {
        GuardFile::path().lock().expect("acquire file guard");
        GuardFile
    }

    fn try_lock() -> Option<Self> {
        match GuardFile::path().try_lock() {
            Ok(()) => Some(GuardFile),
            Err(TryLockError::WouldBlock) => None,
            Err(TryLockError::Error(error)) => {
                panic!("Failed to acquire the file guard: {error}")
            }
        }
    }

    fn unlock(self) {
        drop(self);
    }
}
