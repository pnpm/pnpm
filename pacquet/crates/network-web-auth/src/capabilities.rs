//! Per-capability dependency-injection traits and the production [`Host`]
//! provider for the web-authentication flow.
//!
//! Each trait is a single side effect the TypeScript package injected as a
//! `context` closure (`Date.now`, `setTimeout`, `fetch`, `enquirer.input`,
//! `createReadlineInterface`, `open`). Functions bind only the capabilities
//! they consume, composed on one `Sys` type parameter; production callers
//! turbofish [`Host`], and tests inject `fn`-bound unit-struct fakes. See
//! the "Dependency injection for tests" section of
//! `pacquet/CODE_STYLE_GUIDE.md`.

use std::{
    future::Future,
    io::{self, IsTerminal},
    pin::Pin,
    sync::LazyLock,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::poll_for_web_auth_token::{WebAuthFetchOptions, WebAuthFetchResponse};

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
        // A body read failure is treated the same as `response.json()`
        // rejecting upstream: an empty body makes `token()` fail to parse,
        // so the poll loop retries — while `status` / `retry_after` stay
        // usable for the 202 branch.
        let body = response.text().await.unwrap_or_default();
        Ok(WebAuthFetchResponse { ok, status, retry_after, body })
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

/// Production [`EnterKeyListener::Handle`]. Resolves once the blocking
/// stdin reader observes a line (Enter / EOF); dropping it drops the
/// receiver, abandoning the reader thread's result — the close.
pub struct HostEnterHandle {
    rx: Option<tokio::sync::oneshot::Receiver<()>>,
    // The blocking `read_line` cannot be force-cancelled, but dropping the
    // join handle detaches it and the dropped receiver ignores its result.
    _reader: tokio::task::JoinHandle<()>,
}

impl Future for HostEnterHandle {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let Some(rx) = this.rx.as_mut() else { return Poll::Pending };
        match Pin::new(rx).poll(cx) {
            Poll::Pending => Poll::Pending,
            // Enter pressed (or EOF): the reader sent.
            Poll::Ready(Ok(())) => {
                this.rx = None;
                Poll::Ready(())
            }
            // Sender dropped without sending (read error): never fire, so
            // the browser is not opened spuriously.
            Poll::Ready(Err(_)) => {
                this.rx = None;
                Poll::Pending
            }
        }
    }
}

impl EnterKeyListener for Host {
    type Handle = HostEnterHandle;

    fn listen() -> io::Result<HostEnterHandle> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let reader = tokio::task::spawn_blocking(move || {
            let mut line = String::new();
            if io::stdin().read_line(&mut line).is_ok() {
                let _ = tx.send(());
            }
        });
        Ok(HostEnterHandle { rx: Some(rx), _reader: reader })
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
mod tests {
    use super::{Clock, Host, StdinIsTty, StdoutIsTty};

    /// The clock reads a real, post-epoch wall-clock value.
    #[test]
    fn host_clock_reads_a_non_zero_time() {
        let now = Host::now_ms();
        eprintln!("Host::now_ms() = {now}");
        assert!(now > 0);
    }

    /// The TTY probes are dispatchable and return a bool. The value
    /// depends on how the test harness wired stdio, so only its type is
    /// asserted — the behavioral branches are covered by fakes in the
    /// `prompt_browser_open` / `with_otp_handling` tests.
    #[test]
    fn host_tty_probes_are_callable() {
        let _: bool = Host::stdin_is_tty();
        let _: bool = Host::stdout_is_tty();
    }
}
