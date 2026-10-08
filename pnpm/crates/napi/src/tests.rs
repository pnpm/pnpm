use crate::read_config::{ReadConfigOptions, read_config};

#[test]
fn addon_entry_point_configures_rayon_pool() {
    const CHILD: &str = "PNPM_NAPI_RAYON_POOL_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::addon_entry_point_configures_rayon_pool"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let config = read_config(ReadConfigOptions {
        dir: temp
            .path()
            .to_str()
            .unwrap()
            .to_string(),
    })
    .expect("read_config must succeed");
    let _ = config.registries;
    let current_threads = rayon::current_num_threads();
    let override_threads = std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());
    if let Some(expected) = override_threads {
        assert_eq!(current_threads, expected);
    } else {
        let parallelism =
            std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        let expected = pnpm_package_manager::rayon_pool_size(
            parallelism,
            pnpm_package_manager::RAYON_THREADS_PER_CORE,
        );
        assert_eq!(current_threads, expected);
    }
}
