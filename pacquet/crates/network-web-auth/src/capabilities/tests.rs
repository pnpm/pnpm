use super::{Clock, Host, StdinIsTty, StdoutIsTty};

/// `0` is `Host::now_ms`'s pre-epoch fallback, so a non-zero read confirms
/// the real wall clock was queried rather than the fallback.
#[test]
fn host_clock_reads_a_non_zero_time() {
    let now = Host::now_ms();
    eprintln!("Host::now_ms() = {now}");
    assert!(now > 0);
}

/// The TTY probes are dispatchable and return a bool. The value depends on
/// how the test harness wired stdio, so only its type is asserted — the
/// behavioral branches are covered by fakes in the `prompt_browser_open` /
/// `with_otp_handling` tests.
#[test]
fn host_tty_probes_are_callable() {
    let _: bool = Host::stdin_is_tty();
    let _: bool = Host::stdout_is_tty();
}
