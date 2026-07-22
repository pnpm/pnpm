use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

use pretty_assertions::assert_eq;

use super::{
    Clock, EnterListenerState, Host, HostEnterHandle, StdinIsTty, StdoutIsTty, TOKEN_BODY_LIMIT,
    WebAuthFetch,
};
use crate::poll_for_web_auth_token::WebAuthFetchOptions;

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

#[tokio::test]
async fn host_fetch_reads_a_token_body_within_the_cap() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/done")
        .with_status(200)
        .with_body(r#"{"token":"tok"}"#)
        .create_async()
        .await;

    let response = Host::fetch(&format!("{}/done", server.url()), &WebAuthFetchOptions::default())
        .await
        .expect("a response");

    assert!(response.ok, "got {response:?}");
    assert_eq!(response.status, 200);
    assert!(!response.truncated, "a within-cap body is not truncated");
    assert_eq!(response.token().expect("parse the body"), Some("tok".to_owned()));
}

/// A body with an invalid UTF-8 byte is decoded losslessly (the bad byte
/// becomes U+FFFD) so an otherwise-parsable token still comes through,
/// matching the TypeScript side's non-fatal `TextDecoder`. A strict decode
/// would yield an empty body and no token, diverging from TypeScript.
#[tokio::test]
async fn host_fetch_decodes_an_invalid_utf8_token_body_lossily() {
    let mut server = mockito::Server::new_async().await;
    let mut body = br#"{"token":"a"#.to_vec();
    body.push(0xFF);
    body.extend_from_slice(br#"b"}"#);
    server.mock("GET", "/done").with_status(200).with_body(body).create_async().await;

    let response = Host::fetch(&format!("{}/done", server.url()), &WebAuthFetchOptions::default())
        .await
        .expect("a response");

    assert!(response.ok, "got {response:?}");
    assert!(!response.truncated);
    assert_eq!(
        response.token().expect("parse the lossily-decoded body"),
        Some("a\u{FFFD}b".to_owned())
    );
}

/// A leading UTF-8 BOM is stripped before parsing, matching the TypeScript
/// side's `TextDecoder`. `serde_json` rejects a BOM, so without stripping an
/// otherwise-valid token body would be dropped and the poll would time out.
#[tokio::test]
async fn host_fetch_strips_a_leading_bom_from_the_token_body() {
    let mut server = mockito::Server::new_async().await;
    let mut body = vec![0xEF, 0xBB, 0xBF];
    body.extend_from_slice(br#"{"token":"tok"}"#);
    server.mock("GET", "/done").with_status(200).with_body(body).create_async().await;

    let response = Host::fetch(&format!("{}/done", server.url()), &WebAuthFetchOptions::default())
        .await
        .expect("a response");

    assert!(response.ok, "got {response:?}");
    assert!(!response.truncated);
    assert_eq!(response.token().expect("parse the BOM-prefixed body"), Some("tok".to_owned()));
}

#[tokio::test]
async fn host_fetch_marks_a_token_body_larger_than_the_cap_truncated() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/done")
        .with_status(200)
        .with_body(format!(r#"{{"token":"{}"}}"#, "a".repeat(TOKEN_BODY_LIMIT)))
        .create_async()
        .await;

    let response = Host::fetch(&format!("{}/done", server.url()), &WebAuthFetchOptions::default())
        .await
        .expect("a response");

    assert!(response.ok, "got {response:?}");
    assert!(response.truncated, "an over-cap body must be reported truncated");
    assert_eq!(response.token().expect("truncation short-circuits parsing"), None);
}

#[tokio::test]
async fn host_fetch_skips_the_body_of_a_202_response() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/done")
        .with_status(202)
        .with_header("retry-after", "5")
        .with_body("x".repeat(1024))
        .create_async()
        .await;

    let response = Host::fetch(&format!("{}/done", server.url()), &WebAuthFetchOptions::default())
        .await
        .expect("a response");

    assert!(response.ok, "got {response:?}");
    assert_eq!(response.status, 202);
    assert_eq!(response.retry_after.as_deref(), Some("5"));
    assert_eq!(response.body, "");
    assert!(!response.truncated);
}

#[tokio::test]
async fn host_fetch_skips_the_body_of_a_non_ok_response() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/done")
        .with_status(404)
        .with_body("not found, at length")
        .create_async()
        .await;

    let response = Host::fetch(&format!("{}/done", server.url()), &WebAuthFetchOptions::default())
        .await
        .expect("a response");

    assert!(!response.ok, "got {response:?}");
    assert_eq!(response.status, 404);
    assert_eq!(response.body, "");
    assert!(!response.truncated);
}

fn poll_handle(handle: &mut HostEnterHandle) -> Poll<()> {
    let mut cx = Context::from_waker(Waker::noop());
    Pin::new(handle).poll(&mut cx)
}

#[test]
fn enter_handle_stays_ready_once_completed() {
    let (tx, enter) = tokio::sync::oneshot::channel();
    let mut handle = HostEnterHandle {
        enter,
        state: EnterListenerState::Waiting,
        cancel: Arc::new(AtomicBool::new(false)),
    };

    assert_eq!(poll_handle(&mut handle), Poll::Pending);
    tx.send(()).expect("the receiver is alive");
    assert_eq!(poll_handle(&mut handle), Poll::Ready(()));
    // Re-polls resolve from the terminal state without touching the spent
    // oneshot receiver.
    assert_eq!(poll_handle(&mut handle), Poll::Ready(()));
}

#[test]
fn enter_handle_never_resolves_after_a_reader_error() {
    let (tx, enter) = tokio::sync::oneshot::channel::<()>();
    let mut handle = HostEnterHandle {
        enter,
        state: EnterListenerState::Waiting,
        cancel: Arc::new(AtomicBool::new(false)),
    };

    // The reader thread exiting without signalling drops the sender.
    drop(tx);
    assert_eq!(poll_handle(&mut handle), Poll::Pending);
    assert_eq!(poll_handle(&mut handle), Poll::Pending);
}

#[test]
fn enter_handle_drop_sets_the_cancel_flag() {
    let (_tx, enter) = tokio::sync::oneshot::channel::<()>();
    let cancel = Arc::new(AtomicBool::new(false));
    let handle =
        HostEnterHandle { enter, state: EnterListenerState::Waiting, cancel: Arc::clone(&cancel) };

    drop(handle);
    assert!(cancel.load(Ordering::Relaxed));
}
