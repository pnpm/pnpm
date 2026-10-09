use super::{configure_rayon_pool, rayon_pool_size};

#[test]
fn rayon_pool_is_the_scaled_parallelism_between_4_and_16_threads() {
    let parallelism = [1, 2, 3, 4, 8, 9, 32, 256];
    assert_eq!(parallelism.map(|cores| rayon_pool_size(cores, 2)), [4, 4, 6, 8, 16, 16, 16, 16]);
    assert_eq!(parallelism.map(|cores| rayon_pool_size(cores, 1)), [4, 4, 4, 4, 8, 9, 16, 16]);
}

#[test]
fn configure_rayon_pool_is_idempotent() {
    const CHILD: &str = "PNPM_RAYON_POOL_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "rayon_pool::tests::configure_rayon_pool_is_idempotent"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        return;
    }

    configure_rayon_pool();
    configure_rayon_pool();
    let current_threads = rayon::current_num_threads();
    let parallelism = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let override_threads = std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());
    let expected = match override_threads {
        Some(0) => parallelism,
        Some(threads) => threads,
        None => rayon_pool_size(parallelism, super::RAYON_THREADS_PER_CORE),
    };
    assert_eq!(current_threads, expected);
}
