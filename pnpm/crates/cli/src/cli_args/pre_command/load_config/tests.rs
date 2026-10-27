use std::{cell::RefCell, path::PathBuf};

std::thread_local! {
    static TEST_GLOBAL_CONFIG_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub(super) fn current_test_global_config_dir() -> Option<PathBuf> {
    TEST_GLOBAL_CONFIG_DIR.with(|cell| cell.borrow().clone())
}

pub(in crate::cli_args) fn with_test_global_config_dir<ResultValue>(
    dir: Option<PathBuf>,
    run: impl FnOnce() -> ResultValue,
) -> ResultValue {
    TEST_GLOBAL_CONFIG_DIR.with(|cell| {
        let prev = cell.replace(dir);
        let result = run();
        cell.replace(prev);
        result
    })
}
