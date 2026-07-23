use pacquet_reporter::SilentReporter;

use super::{AddUserError, ClassicLoginOpError, add_user_error_to_op};

/// A transport failure of the classic `PUT` rewraps into
/// `ClassicLoginOpError::Transport`. Unlike the other arms, this one is not
/// reachable end-to-end: the classic `PUT` and the web-login `POST` share a
/// host, so a mock registry can't answer the `POST` yet fail only the `PUT`, and
/// the requests bypass the `Sys` fetch seam — so the pure mapping is asserted
/// directly.
#[test]
fn transport_error_maps_to_op_transport() {
    let op = add_user_error_to_op::<SilentReporter>(AddUserError::Transport {
        reason: "connection refused".to_owned(),
    });
    let ClassicLoginOpError::Transport { reason } = op else {
        panic!("expected Transport, got {op:?}");
    };
    assert_eq!(reason, "connection refused");
}
