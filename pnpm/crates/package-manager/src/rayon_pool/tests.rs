use super::{MAX_RAYON_THREADS, MIN_RAYON_THREADS, configure_rayon_pool, rayon_pool_size};

#[test]
fn rayon_pool_is_the_scaled_parallelism_between_4_and_16_threads() {
    let parallelism = [1, 2, 3, 4, 8, 9, 32, 256];
    assert_eq!(parallelism.map(|cores| rayon_pool_size(cores, 2)), [4, 4, 6, 8, 16, 16, 16, 16]);
    assert_eq!(parallelism.map(|cores| rayon_pool_size(cores, 1)), [4, 4, 4, 4, 8, 9, 16, 16]);
}

#[test]
fn configure_rayon_pool_is_idempotent() {
    configure_rayon_pool();
    configure_rayon_pool();
    let current_threads = rayon::current_num_threads();
    assert!(current_threads >= MIN_RAYON_THREADS);
    assert!(current_threads <= MAX_RAYON_THREADS);
}
