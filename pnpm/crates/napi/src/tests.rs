use crate::read_config::{
    ReadConfigOptions,
    read_config,
};
use command_extra::CommandExtra;

#[test]
fn addon_entry_point_configures_rayon_pool() {
    const CHILD: &str = "PNPM_NAPI_RAYON_POOL_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .with_args(["--exact", "tests::addon_entry_point_configures_rayon_pool"])
            .with_env(CHILD, "1")
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
    let parallelism =
        std::thread::available_parallelism().map(std::num::NonZeroUsize::get).unwrap_or(1);
    let override_threads = std::env::var("RAYON_NUM_THREADS")
        .ok()
        .map(|value| value.parse::<usize>())
        .and_then(Result::ok);
    let expected = match override_threads {
        Some(0) => parallelism,
        Some(threads) => threads,
        None => pnpm_package_manager::rayon_pool_size(
            parallelism,
            pnpm_package_manager::RAYON_THREADS_PER_CORE,
        ),
    };
    assert_eq!(current_threads, expected);
}
