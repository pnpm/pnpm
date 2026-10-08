use crate::read_config::{ReadConfigOptions, read_config};

#[test]
fn addon_entry_point_configures_rayon_pool() {
    let temp = tempfile::tempdir().unwrap();
    let _ = read_config(ReadConfigOptions {
        dir: temp
            .path()
            .to_str()
            .unwrap()
            .to_string(),
    });
    let current_threads = rayon::current_num_threads();
    assert!(current_threads >= pnpm_package_manager::MIN_RAYON_THREADS);
    assert!(current_threads <= pnpm_package_manager::MAX_RAYON_THREADS);
}
