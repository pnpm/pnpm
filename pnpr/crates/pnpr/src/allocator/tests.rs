use super::{MI_OPTION_PURGE_DELAY, mi_option_get, purge_freed_memory_immediately};
use std::{env, process::Command};

const EXPECTED_PURGE_DELAY: &str = "PNPR_TEST_EXPECTED_PURGE_DELAY";

// mimalloc reads `MIMALLOC_PURGE_DELAY` once per process, so each case
// reruns this test in a child process. The override case also proves
// that `MI_OPTION_PURGE_DELAY` indexes the option that variable sets.
#[test]
fn purge_delay_is_zero_unless_the_environment_sets_it() {
    if let Ok(expected) = env::var(EXPECTED_PURGE_DELAY) {
        purge_freed_memory_immediately();
        // SAFETY: `mimalloc` links the library that defines this
        // function, and the index is a valid option.
        let purge_delay = unsafe { mi_option_get(MI_OPTION_PURGE_DELAY) };
        assert_eq!(purge_delay.to_string(), expected);
        return;
    }
    for (setting, expected) in [(None, "0"), (Some("1234"), "1234")] {
        let mut child = Command::new(env::current_exe().expect("test binary path"));
        child
            .args([
                "--exact",
                "allocator::tests::purge_delay_is_zero_unless_the_environment_sets_it",
            ])
            .env(EXPECTED_PURGE_DELAY, expected)
            .env_remove("MIMALLOC_PURGE_DELAY");
        if let Some(setting) = setting {
            child.env("MIMALLOC_PURGE_DELAY", setting);
        }
        let output = child.output().expect("run the test binary");
        let stdout = String::from_utf8_lossy(&output.stdout);
        eprintln!("MIMALLOC_PURGE_DELAY={setting:?}, expecting {expected}:\n{stdout}");
        assert!(output.status.success());
        assert!(stdout.contains("1 passed"));
    }
}
