use super::SingleThreaded;

/// Pins that the witness is claimable once, which is what keeps a caller
/// from minting one after the process has spawned a thread.
///
/// One test, not two, because the claim is process-global and `cargo test`
/// runs a module's tests in one process: a second test could not tell a
/// refusal from this one's claim.
#[test]
fn the_witness_is_claimable_once_and_sets_a_variable() {
    let witness = SingleThreaded::claim().expect("the first claim succeeds");
    assert!(SingleThreaded::claim().is_none(), "a second claim is refused");

    witness.set_var("PNPM_FFI_PROCESS_TEST_VAR", "set");
    assert_eq!(std::env::var("PNPM_FFI_PROCESS_TEST_VAR").as_deref(), Ok("set"));
}
