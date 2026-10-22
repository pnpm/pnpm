use std::collections::HashSet;

use super::{MAX_DECODE_THREADS, run_decode};

#[tokio::test(flavor = "multi_thread")]
async fn every_decode_runs_on_the_same_few_threads() {
    let mut threads = HashSet::new();
    for _ in 0..64 {
        let name = run_decode(|| std::thread::current().name().map(str::to_owned))
            .await
            .expect("run the decode")
            .expect("decode threads are named");
        assert!(name.starts_with("pnpm-metadata-decode-"), "{name}");
        threads.insert(name);
    }
    assert!(threads.len() <= MAX_DECODE_THREADS, "{threads:?}");
}

#[tokio::test(flavor = "multi_thread")]
#[should_panic(expected = "decode failed")]
async fn a_panicking_decode_panics_in_the_caller() {
    let _ = run_decode(|| panic!("decode failed")).await;
}
