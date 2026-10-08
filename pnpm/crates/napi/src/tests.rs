#[test]
fn addon_sizes_rayon_pool_within_bounds() {
    pnpm_package_manager::configure_rayon_pool();
    let current_threads = rayon::current_num_threads();
    assert!(current_threads >= pnpm_package_manager::MIN_RAYON_THREADS);
    assert!(current_threads <= pnpm_package_manager::MAX_RAYON_THREADS);
}
