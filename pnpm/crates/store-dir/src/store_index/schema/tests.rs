use crate::{StoreIndex, store_index::tests::sample_index};
use std::{path::Path, sync::Barrier};

#[test]
fn concurrent_first_open_persists_every_writer_row() {
    for _ in 0..16 {
        let store = tempfile::tempdir().unwrap();
        open_concurrently(store.path());
    }
}

fn open_concurrently(store: &Path) {
    let start = Barrier::new(8);
    std::thread::scope(|scope| {
        let writers = (0..8)
            .map(|worker| {
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    let index =
                        StoreIndex::open(store).expect("initialize a concurrent store writer");
                    index
                        .set(&format!("fixture-{worker}"), &sample_index())
                        .unwrap();
                })
            })
            .collect::<Vec<_>>();
        for writer in writers {
            writer.join().unwrap();
        }
    });
    let index = StoreIndex::open_readonly(store).unwrap();
    for worker in 0..8 {
        assert_eq!(
            index
                .get(&format!("fixture-{worker}"))
                .unwrap(),
            Some(sample_index()),
        );
    }
}
