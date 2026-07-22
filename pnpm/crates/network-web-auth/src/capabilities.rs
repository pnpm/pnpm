//! Per-capability dependency-injection traits and the production [`Host`]
//! provider for the web-authentication flow.
//!
//! Each trait is a single side effect the TypeScript package injected as a
//! `context` closure (`Date.now`, `setTimeout`, `fetch`, `enquirer.input`,
//! `createReadlineInterface`, `open`). Functions bind only the capabilities
//! they consume, composed on one `Sys` type parameter; production callers
//! turbofish [`Host`], and tests inject `fn`-bound unit-struct fakes. See
//! the "Dependency injection for tests" section of
//! `pnpm/CODE_STYLE_GUIDE.md`.

use std::{
    future::Future,
    io::{self, IsTerminal},
    pin::Pin,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use pacquet_network::{LimitedBody, read_limited_body};

use crate::poll_for_web_auth_token::{
    WebAuthFetchOptions, WebAuthFetchResponse, body_may_carry_token,
};

/// Read the current wall-clock time as Unix-epoch milliseconds.
///
/// Mirrors TS `Date.now()`. The poll loop measures elapsed time against a
/// timeout and stamps [`WebAuthTimeoutError`](crate::WebAuthTimeoutError)'s
/// `start_time` / `end_time` with these values, so the unit is
/// milliseconds rather than a `SystemTime`.
pub trait Clock {
    fn now_ms() -> u64;
}

/// Sleep for `ms` milliseconds. Mirrors TS `setTimeout(resolve, ms)`.
pub trait Sleep {
    fn sleep_ms(ms: u64) -> impl Future<Output = ()>;
}

/// Perform the registry "done"-URL poll request. Mirrors TS
/// `fetch(url, options)`: a single `GET` whose response exposes status,
/// headers, and a separately fallible body.
///
/// The `Err` arm stands in for `fetch` itself rejecting (a network
/// failure) — distinct from a successful response whose body fails to
/// parse, which the caller reads from
/// [`WebAuthFetchResponse::token`](crate::WebAuthFetchResponse::token).
pub trait WebAuthFetch {
    fn fetch(
        url: &str,
        options: &WebAuthFetchOptions,
    ) -> impl Future<Output = Result<WebAuthFetchResponse, WebAuthFetchError>>;
}

/// `fetch` itself failed — the request never produced a response. The
/// cause is intentionally opaque: the poll loop treats every fetch failure
/// the same way (swallow and retry on the next tick), so it never inspects
/// this value, matching the TS `catch { continue }`.
#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("web-auth poll request failed")]
#[non_exhaustive]
pub struct WebAuthFetchError;

/// Whether stdin is connected to an interactive terminal. Mirrors TS
/// `process.stdin.isTTY`.
pub trait StdinIsTty {
    fn stdin_is_tty() -> bool;
}

/// Whether stdout is connected to an interactive terminal. Mirrors TS
/// `process.stdout.isTTY`.
pub trait StdoutIsTty {
    fn stdout_is_tty() -> bool;
}

/// Open `url` in the user's default browser. Mirrors the TS `open`
/// package.
pub trait OpenUrl {
    fn open_url(url: &str) -> io::Result<()>;
}

/// Set up an interactive "press Enter" listener on stdin. Mirrors TS
/// `createReadlineInterface` plus its `once('line', ...)` registration.
///
/// [`listen`](Self::listen) returns a [`Handle`](Self::Handle) that is a
/// `Future` resolving once the user presses Enter; **dropping the handle
/// closes the listener** (mirrors `readline.Interface.close`). Setup
/// itself can fail (e.g. raw mode unsupported), which the caller treats as
/// "warn and fall back to plain polling".
pub trait EnterKeyListener {
    type Handle: Future<Output = ()> + Unpin;
    fn listen() -> io::Result<Self::Handle>;
}

/// Prompt the user for a classic one-time password. Mirrors TS
/// `enquirer.input({ message })`: returns the entered string, `None` when
/// the prompt yields no value, or an error.
pub trait PromptOtp {
    fn input(message: &str) -> impl Future<Output = Result<Option<String>, PromptError>>;
}

/// Failure surface of [`PromptOtp::input`].
#[derive(Debug, derive_more::Display, derive_more::Error)]
#[non_exhaustive]
pub enum PromptError {
    /// The user aborted the prompt (Ctrl-C). Mirrors enquirer's
    /// `ExitPromptError`, which the caller handles by re-throwing the
    /// original OTP challenge rather than this error.
    #[display("the one-time password prompt was cancelled")]
    Cancelled,
    /// Any other failure while reading the prompt.
    #[display("failed to read the one-time password prompt: {reason}")]
    Other { reason: String },
}

/// Production implementation of every capability trait in this crate. Each
/// method calls into the real OS facility (`SystemTime`, `tokio`,
/// `reqwest`, `dialoguer`, the `open` crate, stdin).
pub struct Host;

impl Clock for Host {
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
    }
}

impl Sleep for Host {
    async fn sleep_ms(ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

/// The most bytes of a poll response body [`Host::fetch`] reads. The
/// expected body is a small JSON object carrying the token, and the URL it
/// comes from is registry-controlled, so an unbounded read on every poll
/// tick would let a malicious or compromised registry grow memory at will.
const TOKEN_BODY_LIMIT: usize = 64 * 1024;

impl WebAuthFetch for Host {
    async fn fetch(
        url: &str,
        options: &WebAuthFetchOptions,
    ) -> Result<WebAuthFetchResponse, WebAuthFetchError> {
        // One process-wide client so the poll loop reuses connections
        // instead of opening a fresh pool every second.
        static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

        let mut request = CLIENT.get(url);
        if let Some(timeout) = options.timeout {
            request = request.timeout(Duration::from_millis(timeout));
        }
        let response = request.send().await.map_err(|_| WebAuthFetchError)?;
        let ok = response.status().is_success();
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        // The poll loop never reads the body of a non-ok or 202 response;
        // returning early leaves `response` unread, stopping the transfer.
        if !body_may_carry_token(ok, status) {
            return Ok(WebAuthFetchResponse {
                ok,
                status,
                retry_after,
                body: Vec::new(),
                truncated: false,
            });
        }
        // Copy the capped bytes through verbatim; `WebAuthFetchResponse::token`
        // decodes and parses them, so this provider interprets nothing. A read
        // failure yields an empty, untruncated body, which `token` treats the
        // same as an unparsable one (the poll retries); an over-cap body reports
        // `truncated` so `token` reports no token.
        let Ok(LimitedBody { bytes: body, truncated }) =
            read_limited_body(response, TOKEN_BODY_LIMIT).await
        else {
            return Ok(WebAuthFetchResponse {
                ok,
                status,
                retry_after,
                body: Vec::new(),
                truncated: false,
            });
        };
        Ok(WebAuthFetchResponse { ok, status, retry_after, body, truncated })
    }
}

impl StdinIsTty for Host {
    fn stdin_is_tty() -> bool {
        io::stdin().is_terminal()
    }
}

impl StdoutIsTty for Host {
    fn stdout_is_tty() -> bool {
        io::stdout().is_terminal()
    }
}

impl OpenUrl for Host {
    fn open_url(url: &str) -> io::Result<()> {
        // Detached so the call returns as soon as the browser is launched
        // rather than blocking the async task until the launcher exits.
        open::that_detached(url)
    }
}

/// How often the listener thread wakes to re-check the cancel flag. Bounds
/// how long the detached thread outlives a dropped handle.
const ENTER_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Production [`EnterKeyListener::Handle`]. Resolves once the background
/// thread observes an Enter keypress, and stays resolved on re-polls;
/// dropping it sets the cancel flag so that thread stops within one
/// [`ENTER_POLL_INTERVAL`] — crossterm's blocking `event::read()` cannot be
/// cancelled, so the thread polls with a timeout instead of blocking
/// forever.
///
/// Meant to be raced and dropped when another branch wins (e.g. inside a
/// `tokio::select!`): on a stdin read error it deliberately never resolves,
/// so awaiting it on its own would hang.
pub struct HostEnterHandle {
    enter: tokio::sync::oneshot::Receiver<()>,
    state: EnterListenerState,
    cancel: Arc<AtomicBool>,
}

/// Where a [`HostEnterHandle`] is in its lifecycle. `Completed` and
/// `Disabled` are both terminal — the oneshot receiver must not be polled
/// again — but they resolve differently: a completed handle re-polls as
/// `Ready` while a disabled one (stdin read error) stays `Pending` forever
/// so the browser is not opened spuriously.
#[derive(Clone, Copy)]
enum EnterListenerState {
    Waiting,
    Completed,
    Disabled,
}

impl Future for HostEnterHandle {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        match this.state {
            EnterListenerState::Completed => return Poll::Ready(()),
            EnterListenerState::Disabled => return Poll::Pending,
            EnterListenerState::Waiting => {}
        }
        match Pin::new(&mut this.enter).poll(cx) {
            Poll::Pending => Poll::Pending,
            // Enter pressed: the reader thread signalled.
            Poll::Ready(Ok(())) => {
                this.state = EnterListenerState::Completed;
                Poll::Ready(())
            }
            // Reader exited without signalling (read error).
            Poll::Ready(Err(_)) => {
                this.state = EnterListenerState::Disabled;
                Poll::Pending
            }
        }
    }
}

impl Drop for HostEnterHandle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl EnterKeyListener for Host {
    type Handle = HostEnterHandle;

    /// Watch stdin for an Enter keypress without blocking uninterruptibly;
    /// the returned handle documents the cancellation contract. `crossterm`
    /// reads in the terminal's default (cooked) mode — no raw mode — matching
    /// pnpm's plain `readline.createInterface({ input: process.stdin })`,
    /// which reacts to a submitted line rather than individual keys.
    fn listen() -> io::Result<HostEnterHandle> {
        let (tx, enter) = tokio::sync::oneshot::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let reader_cancel = Arc::clone(&cancel);
        thread::Builder::new().name("web-auth-enter-listener".to_owned()).spawn(move || {
            while !reader_cancel.load(Ordering::Relaxed) {
                match event::poll(ENTER_POLL_INTERVAL) {
                    // Input is ready, but skip it without consuming when the
                    // handle was dropped meanwhile — otherwise `read()` would
                    // steal a keystroke from whatever reads stdin next. The
                    // re-check is best-effort: a drop landing between it and
                    // `read()` can still lose one keystroke. That residual
                    // window is a few instructions wide and accepted;
                    // crossterm offers no way to close it short of not
                    // reading stdin at all.
                    Ok(true) => {
                        if reader_cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        // `poll() == Ok(true)` guarantees a complete event is
                        // ready, so `read()` does not block. In cooked mode the
                        // line is submitted on Enter, which maps to `Enter`.
                        match event::read() {
                            Ok(Event::Key(key))
                                if key.code == KeyCode::Enter
                                    && key.kind != KeyEventKind::Release =>
                            {
                                let _ = tx.send(());
                                return;
                            }
                            Ok(_) => {}
                            Err(_) => return,
                        }
                    }
                    // Timed out: loop back to re-check the cancel flag.
                    Ok(false) => {}
                    Err(_) => return,
                }
            }
        })?;
        Ok(HostEnterHandle { enter, state: EnterListenerState::Waiting, cancel })
    }
}

impl PromptOtp for Host {
    async fn input(message: &str) -> Result<Option<String>, PromptError> {
        let message = message.to_owned();
        // `dialoguer` is blocking; keep it off the async runtime.
        tokio::task::spawn_blocking(move || {
            dialoguer::Input::<String>::new().with_prompt(message).allow_empty(true).interact_text()
        })
        .await
        .map_err(|join_error| PromptError::Other { reason: join_error.to_string() })?
        .map(Some)
        .map_err(map_dialoguer_error)
    }
}

fn map_dialoguer_error(error: dialoguer::Error) -> PromptError {
    match error {
        dialoguer::Error::IO(io) if io.kind() == io::ErrorKind::Interrupted => {
            PromptError::Cancelled
        }
        dialoguer::Error::IO(io) => PromptError::Other { reason: io.to_string() },
    }
}

#[cfg(test)]
mod tests;
