//! Coordination for tests that exercise real Windows file removal.

use std::{
    collections::{HashMap, hash_map::Entry},
    io,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, PoisonError},
};

type Observer = Arc<Mutex<Box<dyn FnMut(&io::Result<()>) + Send>>>;

static OBSERVERS: LazyLock<Mutex<HashMap<PathBuf, Observer>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Observe completed removal attempts for exactly `path` while `operation` runs.
///
/// The observer runs synchronously after the real filesystem call, including
/// attempts on worker threads. It must not recursively remove the same path
/// or wait for another observer to run.
/// Registration is removed when the operation returns or unwinds.
pub fn with_file_removal_observer<Output>(
    path: &Path,
    observer: impl FnMut(&io::Result<()>) + Send + 'static,
    operation: impl FnOnce() -> Output,
) -> Output {
    let path = path.to_path_buf();
    {
        let mut observers = OBSERVERS.lock().unwrap_or_else(PoisonError::into_inner);
        match observers.entry(path.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(Arc::new(Mutex::new(Box::new(observer))));
            }
            Entry::Occupied(_) => panic!("a removal observer is already registered for {path:?}"),
        }
    }
    let _registration = Registration(path);
    operation()
}

struct Registration(PathBuf);

impl Drop for Registration {
    fn drop(&mut self) {
        let observer = OBSERVERS.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.0);
        drop(observer);
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn notify_file_removal(path: &Path, result: &io::Result<()>) {
    let observer =
        OBSERVERS.lock().unwrap_or_else(PoisonError::into_inner).get(path).map(Arc::clone);
    if let Some(observer) = observer {
        observer.lock().unwrap_or_else(PoisonError::into_inner)(result);
    }
}
