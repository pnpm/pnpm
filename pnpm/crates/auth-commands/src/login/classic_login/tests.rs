use pnpm_reporter::SilentReporter;

use super::{AddUserError, ClassicLoginOpError, Credentials, add_user, add_user_error_to_op};

#[tokio::test]
async fn classic_login_does_not_forward_credentials_on_body_preserving_redirects() {
    let mut registry = mockito::Server::new_async().await;
    let mut destination = mockito::Server::new_async().await;
    let credentials =
        Credentials { username: "john", password: "secret", email: "john@example.com" };
    let redirected_request = destination
        .mock("PUT", "/stolen")
        .expect(0)
        .create_async()
        .await;

    for status in [307, 308] {
        let redirect = registry
            .mock("PUT", "/-/user/org.couchdb.user:john")
            .with_status(status)
            .with_header("location", &format!("{}/stolen", destination.url()))
            .create_async()
            .await;

        let error = add_user(&crate::login::support::client(), &registry.url(), credentials, None)
            .await
            .expect_err("login redirect must fail");
        assert!(
            matches!(error, AddUserError::Http { status: actual, .. } if usize::from(actual) == status),
        );
        redirect.assert_async().await;
        redirect.remove_async().await;
    }
    redirected_request.assert_async().await;
}

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
