use std::{
    cell::{Cell, RefCell},
    future::{self, Future},
    io,
    pin::Pin,
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

/// Expand the per-test browser-open fake at the top of a `#[tokio::test]`
/// body.
///
/// Invoked as `browser_fake!();`, it declares — as items local to the test
/// function — the `STDIN_TTY` / `LISTEN_OUTCOME` / `OPEN_OUTCOME` /
/// `OPEN_CALLS` / `CLOSED` / `ENTER_TX` / `EMITTED` thread-locals, a unit
/// `Fake` implementing [`StdinIsTty`], [`OpenUrl`], and [`EnterKeyListener`]
/// over them, a `RecordingReporter`, and the `reset` / `set_*` /
/// `simulate_enter` / `open_calls` / `closed` / `infos` / `warns` /
/// `messages_at` helpers. Because the state lives inside the test function,
/// every test gets its own storage; nothing is shared at module scope, so
/// concurrently running tests can never race on it. This is the "state in a
/// `static` inside the `#[test]` body" rule of the "Dependency injection for
/// tests" section of `pnpm/CODE_STYLE_GUIDE.md`.
///
/// The generated helpers carry `#[allow(dead_code)]`: every expansion emits
/// the full fake surface, but each test drives only the pieces its scenario
/// needs.
macro_rules! browser_fake {
    () => {
        thread_local! {
            static STDIN_TTY: Cell<bool> = const { Cell::new(true) };
            static LISTEN_OUTCOME: Cell<Outcome> = const { Cell::new(Outcome::Succeed) };
            static OPEN_OUTCOME: Cell<Outcome> = const { Cell::new(Outcome::Succeed) };
            static OPEN_CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
            static CLOSED: Cell<bool> = const { Cell::new(false) };
            static ENTER_TX: RefCell<Option<oneshot::Sender<()>>> = const { RefCell::new(None) };
            static EMITTED: RefCell<Vec<(LogLevel, String)>> = const { RefCell::new(Vec::new()) };
        }

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

        /// Resolves when the test simulates an Enter keypress; sets the
        /// `CLOSED` flag on drop, standing in for `readline.Interface.close`.
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
                if let LogEvent::Global(GlobalLog { level, message }) = event {
                    EMITTED.with(|emitted| emitted.borrow_mut().push((*level, message.clone())));
                }
            }
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn reset() {
            STDIN_TTY.with(|tty| tty.set(true));
            LISTEN_OUTCOME.with(|outcome| outcome.set(Outcome::Succeed));
            OPEN_OUTCOME.with(|outcome| outcome.set(Outcome::Succeed));
            OPEN_CALLS.with(|calls| calls.borrow_mut().clear());
            CLOSED.with(|closed| closed.set(false));
            ENTER_TX.with(|cell| *cell.borrow_mut() = None);
            EMITTED.with(|emitted| emitted.borrow_mut().clear());
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn set_stdin_tty(is_tty: bool) {
            STDIN_TTY.with(|tty| tty.set(is_tty));
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn set_listen_outcome(outcome: Outcome) {
            LISTEN_OUTCOME.with(|cell| cell.set(outcome));
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn set_open_outcome(outcome: Outcome) {
            OPEN_OUTCOME.with(|cell| cell.set(outcome));
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn simulate_enter() {
            if let Some(tx) = ENTER_TX.with(|cell| cell.borrow_mut().take()) {
                let _ = tx.send(());
            }
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn open_calls() -> Vec<String> {
            OPEN_CALLS.with(|calls| calls.borrow().clone())
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn closed() -> bool {
            CLOSED.with(Cell::get)
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn infos() -> Vec<String> {
            messages_at(LogLevel::Info)
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn warns() -> Vec<String> {
            messages_at(LogLevel::Warn)
        }

        #[allow(dead_code, reason = "macro emits the full fake surface; tests use a subset")]
        fn messages_at(level: LogLevel) -> Vec<String> {
            EMITTED.with(|emitted| {
                emitted
                    .borrow()
                    .iter()
                    .filter(|(emitted_level, _)| *emitted_level == level)
                    .map(|(_, message)| message.clone())
                    .collect()
            })
        }
    };
}

/// Poll error type. Carries a message so the rejection tests can assert
/// the propagated value.
#[derive(Debug)]
struct PollError(&'static str);

const AUTH_URL: &str = "https://example.com/auth";

#[tokio::test]
async fn returns_the_poll_result_when_poll_completes_before_enter_keypress() {
    browser_fake!();
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
    browser_fake!();
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
    browser_fake!();
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
    browser_fake!();
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
    browser_fake!();
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
    browser_fake!();
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
    browser_fake!();
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
    browser_fake!();
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
