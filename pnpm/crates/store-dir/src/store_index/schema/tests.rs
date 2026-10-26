use crate::StoreIndex;
use std::{path::Path, sync::Barrier};

#[test]
fn concurrent_first_open_keeps_every_writer_available() {
    for _ in 0..16 {
        let store = tempfile::tempdir().unwrap();
        open_concurrently(store.path());
    }
}

fn open_concurrently(store: &Path) {
    let start = Barrier::new(8);
    std::thread::scope(|scope| {
        let writers = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    StoreIndex::open(store).expect("initialize a concurrent store writer")
                })
            })
            .collect::<Vec<_>>();
        for writer in writers {
            drop(writer.join().unwrap());
        }
    });
}
