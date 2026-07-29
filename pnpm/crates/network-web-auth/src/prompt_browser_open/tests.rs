use std::{
    cell::{Cell, RefCell},
    future::{self, Future},
    io,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pretty_assertions::assert_eq;
use tokio::{sync::oneshot, task::LocalSet};

use super::prompt_browser_open;
use crate::capabilities::{EnterKeyListener, OpenUrl, StdinIsTty};

#[derive(Clone, Copy)]
enum Outcome {
    Succeed,
    Fail,
}

// Per-test fake for stdin-tty / browser-open / enter-key, plus a recording
// reporter. Its state is fn-local — thread-locals for the inputs and a `static
// Mutex<Vec<LogEvent>>` for the captured log events — so each `#[test]` gets
// independent storage and concurrent tests never share it. Each test names the
// optional helpers it drives, so every emitted helper is used and none needs a
// `dead_code` allow.
macro_rules! browser_fake {
    ($($helper:ident),* $(,)?) => {
        thread_local! {
            static STDIN_TTY: Cell<bool> = const { Cell::new(true) };
            static LISTEN_OUTCOME: Cell<Outcome> = const { Cell::new(Outcome::Succeed) };
            static OPEN_OUTCOME: Cell<Outcome> = const { Cell::new(Outcome::Succeed) };
            static OPEN_CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
            static CLOSED: Cell<bool> = const { Cell::new(false) };
            static ENTER_TX: RefCell<Option<oneshot::Sender<()>>> = const { RefCell::new(None) };
        }
        static EMITTED: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

        struct Fake;

        impl StdinIsTty for Fake {
            fn stdin_is_tty() -> bool {
                STDIN_TTY.with(Cell::get)
            }
        }

        impl OpenUrl for Fake {
            fn open_url(url: &str) -> io::Result<()> {
                OPEN_CALLS.with(|calls| calls.borrow_mut().push(url.to_owned()));
                match OPEN_OUTCOME.with(Cell::get) {
                    Outcome::Succeed => Ok(()),
                    Outcome::Fail => Err(io::Error::other("xdg-open not found")),
                }
            }
        }

        // Resolves when the test simulates an Enter keypress; sets the
        // `CLOSED` flag on drop, standing in for `readline.Interface.close`.
        struct FakeEnterHandle {
            rx: oneshot::Receiver<()>,
        }

        impl Future for FakeEnterHandle {
            type Output = ();

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
                Pin::new(&mut self.get_mut().rx).poll(cx).map(|_| ())
            }
        }

        impl Drop for FakeEnterHandle {
            fn drop(&mut self) {
                CLOSED.with(|closed| closed.set(true));
            }
        }

        impl EnterKeyListener for Fake {
            type Handle = FakeEnterHandle;

            fn listen() -> io::Result<FakeEnterHandle> {
                match LISTEN_OUTCOME.with(Cell::get) {
                    Outcome::Fail => Err(io::Error::other("setRawMode not supported")),
                    Outcome::Succeed => {
                        let (tx, rx) = oneshot::channel();
                        ENTER_TX.with(|cell| *cell.borrow_mut() = Some(tx));
                        Ok(FakeEnterHandle { rx })
                    }
                }
            }
        }

        struct RecordingReporter;

        impl Reporter for RecordingReporter {
            fn emit(event: &LogEvent) {
                EMITTED.lock().expect("EMITTED not poisoned").push(event.clone());
            }
        }

        fn reset() {
            STDIN_TTY.with(|tty| tty.set(true));
            LISTEN_OUTCOME.with(|outcome| outcome.set(Outcome::Succeed));
            OPEN_OUTCOME.with(|outcome| outcome.set(Outcome::Succeed));
            OPEN_CALLS.with(|calls| calls.borrow_mut().clear());
            CLOSED.with(|closed| closed.set(false));
            ENTER_TX.with(|cell| *cell.borrow_mut() = None);
            EMITTED.lock().expect("EMITTED not poisoned").clear();
        }

        $( browser_fake!(@helper $helper); )*
    };

    (@helper set_stdin_tty) => {
        fn set_stdin_tty(is_tty: bool) {
            STDIN_TTY.with(|tty| tty.set(is_tty));
        }
    };
    (@helper set_listen_outcome) => {
        fn set_listen_outcome(outcome: Outcome) {
            LISTEN_OUTCOME.with(|cell| cell.set(outcome));
        }
    };
    (@helper set_open_outcome) => {
        fn set_open_outcome(outcome: Outcome) {
            OPEN_OUTCOME.with(|cell| cell.set(outcome));
        }
    };
    (@helper simulate_enter) => {
        fn simulate_enter() {
            if let Some(tx) = ENTER_TX.with(|cell| cell.borrow_mut().take()) {
                let _ = tx.send(());
            }
        }
    };
    (@helper open_calls) => {
        fn open_calls() -> Vec<String> {
            OPEN_CALLS.with(|calls| calls.borrow().clone())
        }
    };
    (@helper closed) => {
        fn closed() -> bool {
            CLOSED.with(Cell::get)
        }
    };
    (@helper infos) => {
        fn infos() -> Vec<String> {
            global_messages_at(&EMITTED.lock().expect("EMITTED not poisoned"), LogLevel::Info)
        }
    };
    (@helper warns) => {
        fn warns() -> Vec<String> {
            global_messages_at(&EMITTED.lock().expect("EMITTED not poisoned"), LogLevel::Warn)
        }
    };
    (@helper $unknown:ident) => {
        compile_error!(concat!(
            "unknown `browser_fake!` helper `",
            stringify!($unknown),
            "`; expected one of: set_stdin_tty, set_listen_outcome, set_open_outcome, ",
            "simulate_enter, open_calls, closed, infos, warns",
        ));
    };
}

// The `Global`-log messages at `level`, in emit order — the projection that
// the fake's `infos` and `warns` accessors share.
fn global_messages_at(events: &[LogEvent], level: LogLevel) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            LogEvent::Global(GlobalLog { level: emitted, message }) if *emitted == level => {
                Some(message.clone())
            }
            _ => None,
        })
        .collect()
}

/// Poll error type. Carries a message so the rejection tests can assert
/// the propagated value.
#[derive(Debug)]
struct PollError(&'static str);

const AUTH_URL: &str = "https://example.com/auth";

#[tokio::test]
async fn returns_the_poll_result_when_poll_completes_before_enter_keypress() {
    browser_fake!(open_calls, closed);
    reset();

    let token = prompt_browser_open::<Fake, RecordingReporter, PollError, _>(
        AUTH_URL,
        future::ready(Ok("my-token".to_owned())),
    )
    .await
    .expect("a token");

    assert_eq!(token, "my-token");
    assert!(closed(), "the listener should be closed");
    assert!(open_calls().is_empty(), "the browser must not be opened without a keypress");
}

#[tokio::test]
async fn opens_browser_when_enter_key_is_pressed_before_poll_completes() {
    browser_fake!(simulate_enter, open_calls, closed);
    reset();
    LocalSet::new()
        .run_until(async {
            let (poll_tx, poll_rx) = oneshot::channel::<Result<String, PollError>>();
            let handle = tokio::task::spawn_local(async move {
                let poll = async move { poll_rx.await.expect("poll resolved") };
                prompt_browser_open::<Fake, RecordingReporter, PollError, _>(AUTH_URL, poll).await
            });

            // Let the prompt register its listener, then press Enter.
            tokio::task::yield_now().await;
            simulate_enter();
            tokio::task::yield_now().await;

            assert_eq!(open_calls(), vec![AUTH_URL.to_owned()]);

            poll_tx.send(Ok("token-after-enter".to_owned())).expect("send poll result");
            let token = handle.await.expect("join").expect("a token");

            assert_eq!(token, "token-after-enter");
            assert!(closed(), "the listener should be closed");
        })
        .await;
}

#[tokio::test]
async fn warns_and_continues_polling_when_open_fails() {
    browser_fake!(set_open_outcome, simulate_enter, infos, warns);
    reset();
    set_open_outcome(Outcome::Fail);
    LocalSet::new()
        .run_until(async {
            let (poll_tx, poll_rx) = oneshot::channel::<Result<String, PollError>>();
            let handle = tokio::task::spawn_local(async move {
                let poll = async move { poll_rx.await.expect("poll resolved") };
                prompt_browser_open::<Fake, RecordingReporter, PollError, _>(AUTH_URL, poll).await
            });

            tokio::task::yield_now().await;
            simulate_enter();
            tokio::task::yield_now().await;

            assert!(
                warns().iter().any(|message| message.contains("xdg-open not found")),
                "open failure should warn, got {:?}",
                warns(),
            );
            assert!(infos().contains(&"Please open the URL shown above manually.".to_owned()));

            poll_tx.send(Ok("tok".to_owned())).expect("send poll result");
            let token = handle.await.expect("join").expect("a token");
            assert_eq!(token, "tok");
        })
        .await;
}

#[tokio::test]
async fn warns_and_falls_back_to_plain_poll_when_listen_fails() {
    browser_fake!(set_listen_outcome, open_calls, warns);
    reset();
    set_listen_outcome(Outcome::Fail);

    let token = prompt_browser_open::<Fake, RecordingReporter, PollError, _>(
        AUTH_URL,
        future::ready(Ok("fallback-token".to_owned())),
    )
    .await
    .expect("a token");

    assert_eq!(token, "fallback-token");
    assert!(
        warns().iter().any(|message| message.contains("setRawMode not supported")),
        "listener setup failure should warn, got {:?}",
        warns(),
    );
    assert!(open_calls().is_empty());
}

#[tokio::test]
async fn falls_back_to_plain_poll_when_stdin_is_not_a_tty() {
    browser_fake!(set_stdin_tty, open_calls);
    reset();
    set_stdin_tty(false);

    let token = prompt_browser_open::<Fake, RecordingReporter, PollError, _>(
        AUTH_URL,
        future::ready(Ok("plain-token".to_owned())),
    )
    .await
    .expect("a token");

    assert_eq!(token, "plain-token");
    assert!(open_calls().is_empty());
}

#[tokio::test]
async fn shows_the_press_enter_message() {
    browser_fake!(infos);
    reset();

    prompt_browser_open::<Fake, RecordingReporter, PollError, _>(
        AUTH_URL,
        future::ready(Ok("tok".to_owned())),
    )
    .await
    .expect("a token");

    assert!(infos().contains(&"Press ENTER to open the URL in your browser.".to_owned()));
}

#[tokio::test]
async fn does_not_open_browser_for_non_http_auth_url() {
    browser_fake!(open_calls);
    for auth_url in ["javascript:alert(1)", "file:///etc/passwd", "not a url"] {
        reset();

        let token = prompt_browser_open::<Fake, RecordingReporter, PollError, _>(
            auth_url,
            future::ready(Ok("tok".to_owned())),
        )
        .await
        .expect("a token");

        assert_eq!(token, "tok");
        assert!(open_calls().is_empty(), "{auth_url} must not open a browser");
    }
}

#[tokio::test]
async fn cleans_up_when_poll_rejects() {
    browser_fake!(closed);
    reset();

    let error = prompt_browser_open::<Fake, RecordingReporter, PollError, _>(
        AUTH_URL,
        future::ready(Err(PollError("timeout"))),
    )
    .await
    .expect_err("poll rejected");

    assert_eq!(error.0, "timeout");
    assert!(closed(), "the listener should be closed even when the poll rejects");
}
