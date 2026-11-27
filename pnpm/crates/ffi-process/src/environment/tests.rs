use super::SingleThreaded;

/// Pins that the witness is claimable once, which is what keeps a caller
/// from minting one after the process has spawned a thread, and that a
/// write is refused while another thread runs. The test harness runs this
/// test on a thread of its own, so the process is never single-threaded
/// here.
///
/// One test, not several, because the claim is process-global and
/// `cargo test` runs a module's tests in one process: another test could
/// not tell a refusal from this one's claim.
#[test]
fn the_witness_is_claimable_once_and_refuses_a_shared_environment() {
    let witness = SingleThreaded::claim().expect("the first claim succeeds");
    assert!(SingleThreaded::claim().is_none(), "a second claim is refused");

    let written = witness.set_var("PNPM_FFI_PROCESS_TEST_VAR", "set");
    if cfg!(unix) {
        let error = written.expect_err("another thread runs");
        assert!(
            matches!(error, super::SetVarError::OtherThreads { threads, .. } if threads >= 2),
            "{error:?}",
        );
        assert_eq!(std::env::var_os("PNPM_FFI_PROCESS_TEST_VAR"), None);
    } else {
        written.expect("this platform needs no thread check");
        assert_eq!(std::env::var("PNPM_FFI_PROCESS_TEST_VAR").as_deref(), Ok("set"));
    }
}
