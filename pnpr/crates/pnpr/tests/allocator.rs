#![allow(
    clippy::disallowed_methods,
    reason = "test setup starts processes the executor does not run"
)]
use command_extra::CommandExtra;
use std::process::Command;

// The override case also proves that pnpr sets the option
// `MIMALLOC_PURGE_DELAY` names.
#[test]
fn purge_delay_is_zero_unless_the_environment_sets_it() {
    for (setting, expected) in [(None, "0"), (Some("1234"), "1234")] {
        let mut pnpr = Command::new(env!("CARGO_BIN_EXE_pnpr"))
            .with_arg("--allocator-purge-delay")
            .without_env("MIMALLOC_PURGE_DELAY");
        if let Some(setting) = setting {
            pnpr = pnpr.with_env("MIMALLOC_PURGE_DELAY", setting);
        }
        let output = pnpr.output().expect("run pnpr");
        assert!(output.status.success(), "{output:?}");
        let purge_delay = String::from_utf8_lossy(&output.stdout);
        assert_eq!(purge_delay.trim(), expected, "MIMALLOC_PURGE_DELAY={setting:?}");
    }
}
