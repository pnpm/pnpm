//! Shared test fakes for the OTP / web-auth flow
//! ([`pacquet_network_web_auth`]).
//!
//! pnpm's 2FA/OTP/web-auth tests build a mock "context" through a single
//! shared test-helper package, reused by both `publish`'s `otp.test.ts` and
//! `login`'s `login.test.ts`. pacquet mirrors that here: [`FakeHost`]
//! implements every web-auth capability ([`Clock`], [`Sleep`],
//! [`WebAuthFetch`], [`PromptOtp`], [`EnterKeyListener`], the TTY probes, and
//! [`OpenUrl`]) over thread-local state that the `set_*` functions script. The
//! `pacquet-network-web-auth`, `pacquet-publish`, and future `pacquet login`
//! tests drive the same flow against it without a real registry, clock, or
//! TTY.
//!
//! State is thread-local, so concurrently running tests do not see each
//! other's script; call [`reset`] at the start of every test.

use std::{
    cell::{Cell, RefCell},
    future::{self, Future},
    io,
    pin::Pin,
    task::{Context, Poll},
};

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_network_web_auth::{
    Clock, EnterKeyListener, OpenUrl, OtpChallenge, OtpError, OtpErrorBody, PromptError, PromptOtp,
    Sleep, StdinIsTty, StdoutIsTty, WebAuthFetch, WebAuthFetchError, WebAuthFetchOptions,
    WebAuthFetchResponse,
};
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use serde_json::json;
use tokio::sync::oneshot;

/// An operation error that is either an EOTP challenge or a plain failure, so
/// a single error type covers both the OTP and non-OTP paths a fake operation
/// needs to return. Mirrors the shape `isOtpError` reads upstream.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum FakeOtpError {
    #[display("otp challenge")]
    Otp { body: Option<OtpErrorBody> },
    #[display("{_0}")]
    Other(#[error(not(source))] String),
}

impl OtpError for FakeOtpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        match self {
            FakeOtpError::Otp { body } => Some(OtpChallenge { body: body.clone() }),
            FakeOtpError::Other(_) => None,
        }
    }
}

/// What the [`PromptOtp`] fake returns for the classic-OTP prompt.
pub enum InputResponse {
    Value(Option<String>),
    Cancelled,
}

/// How the [`Sleep`] fake advances the fake clock when awaited — left alone,
/// or jumped forward by a fixed number of milliseconds to drive the web-auth
/// poll past its deadline.
#[derive(Clone, Copy)]
pub enum SleepBehavior {
    NoAdvance,
    AdvanceByFixed(u64),
}

/// A scripted sequence of [`WebAuthFetch`] responses: each poll calls it once.
pub type FetchScript = Box<dyn FnMut() -> Result<WebAuthFetchResponse, WebAuthFetchError>>;

thread_local! {
    static STDIN_TTY: Cell<bool> = const { Cell::new(true) };
    static STDOUT_TTY: Cell<bool> = const { Cell::new(true) };
    static TIME: Cell<u64> = const { Cell::new(0) };
    static SLEEP_BEHAVIOR: Cell<SleepBehavior> = const { Cell::new(SleepBehavior::NoAdvance) };
    static FETCH: RefCell<Option<FetchScript>> = const { RefCell::new(None) };
    static INPUT: RefCell<InputResponse> = const { RefCell::new(InputResponse::Value(None)) };
    static ENTER_TX: RefCell<Option<oneshot::Sender<()>>> = const { RefCell::new(None) };
    static EMITTED: RefCell<Vec<(LogLevel, String)>> = const { RefCell::new(Vec::new()) };
}

/// The fake web-auth [host](pacquet_network_web_auth::Host): every capability
/// reads from the thread-local script the `set_*` functions configure.
pub struct FakeHost;

impl StdinIsTty for FakeHost {
    fn stdin_is_tty() -> bool {
        STDIN_TTY.with(Cell::get)
    }
}

impl StdoutIsTty for FakeHost {
    fn stdout_is_tty() -> bool {
        STDOUT_TTY.with(Cell::get)
    }
}

impl Clock for FakeHost {
    fn now_ms() -> u64 {
        TIME.with(Cell::get)
    }
}

impl Sleep for FakeHost {
    fn sleep_ms(ms: u64) -> impl Future<Output = ()> {
        let _ = ms;
        if let SleepBehavior::AdvanceByFixed(jump) = SLEEP_BEHAVIOR.with(Cell::get) {
            TIME.with(|time| time.set(time.get().saturating_add(jump)));
        }
        future::ready(())
    }
}

impl WebAuthFetch for FakeHost {
    fn fetch(
        _url: &str,
        _options: &WebAuthFetchOptions,
    ) -> impl Future<Output = Result<WebAuthFetchResponse, WebAuthFetchError>> {
        let result = FETCH
            .with(|fetch| (fetch.borrow_mut().as_mut().expect("a fetch script must be set"))());
        future::ready(result)
    }
}

impl PromptOtp for FakeHost {
    fn input(_message: &str) -> impl Future<Output = Result<Option<String>, PromptError>> {
        let response = INPUT.with(|input| match &*input.borrow() {
            InputResponse::Value(value) => Ok(value.clone()),
            InputResponse::Cancelled => Err(PromptError::Cancelled),
        });
        future::ready(response)
    }
}

impl OpenUrl for FakeHost {
    fn open_url(_url: &str) -> io::Result<()> {
        Ok(())
    }
}

/// Never resolves on its own — in these tests the web-auth poll always wins or
/// times out before any Enter keypress.
pub struct PendingEnterHandle {
    rx: oneshot::Receiver<()>,
}

impl Future for PendingEnterHandle {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        Pin::new(&mut self.get_mut().rx).poll(cx).map(|_| ())
    }
}

impl EnterKeyListener for FakeHost {
    type Handle = PendingEnterHandle;

    fn listen() -> io::Result<PendingEnterHandle> {
        let (tx, rx) = oneshot::channel();
        ENTER_TX.with(|cell| *cell.borrow_mut() = Some(tx));
        Ok(PendingEnterHandle { rx })
    }
}

/// Records every `pnpm:global` message so a test can assert on the auth URL /
/// warnings the flow surfaces.
pub struct RecordingReporter;

impl Reporter for RecordingReporter {
    fn emit(event: &LogEvent) {
        if let LogEvent::Global(GlobalLog { level, message }) = event {
            EMITTED.with(|emitted| emitted.borrow_mut().push((*level, message.clone())));
        }
    }
}

/// Panics on any global message — the stand-in for the TS `globalWarn` that
/// throws when a test expects no warning.
pub struct UnexpectedReporter;

impl Reporter for UnexpectedReporter {
    fn emit(event: &LogEvent) {
        if let LogEvent::Global(GlobalLog { message, .. }) = event {
            panic!("unexpected global message: {message}");
        }
    }
}

/// Clear every thread-local script back to its default. Call at the start of
/// each test before the `set_*` functions configure the scenario.
pub fn reset() {
    STDIN_TTY.with(|tty| tty.set(true));
    STDOUT_TTY.with(|tty| tty.set(true));
    TIME.with(|time| time.set(0));
    SLEEP_BEHAVIOR.with(|behavior| behavior.set(SleepBehavior::NoAdvance));
    FETCH.with(|fetch| *fetch.borrow_mut() = None);
    INPUT.with(|input| *input.borrow_mut() = InputResponse::Value(None));
    ENTER_TX.with(|cell| *cell.borrow_mut() = None);
    EMITTED.with(|emitted| emitted.borrow_mut().clear());
}

/// Whether [`FakeHost`] reports stdin as a TTY (drives the interactive-prompt
/// gate).
pub fn set_stdin_tty(is_tty: bool) {
    STDIN_TTY.with(|tty| tty.set(is_tty));
}

/// Whether [`FakeHost`] reports stdout as a TTY.
pub fn set_stdout_tty(is_tty: bool) {
    STDOUT_TTY.with(|tty| tty.set(is_tty));
}

/// Set the fake clock (milliseconds) [`FakeHost::now_ms`](Clock::now_ms) reads.
pub fn set_time(ms: u64) {
    TIME.with(|time| time.set(ms));
}

/// Choose how [`FakeHost::sleep_ms`](Sleep::sleep_ms) advances the fake clock.
pub fn set_sleep_behavior(behavior: SleepBehavior) {
    SLEEP_BEHAVIOR.with(|cell| cell.set(behavior));
}

/// Script what the classic-OTP prompt returns.
pub fn set_input(response: InputResponse) {
    INPUT.with(|input| *input.borrow_mut() = response);
}

/// Script the web-auth poll responses.
pub fn set_fetch(script: FetchScript) {
    FETCH.with(|fetch| *fetch.borrow_mut() = Some(script));
}

/// The `pnpm:global` info messages [`RecordingReporter`] captured.
#[must_use]
pub fn infos() -> Vec<String> {
    messages_at(LogLevel::Info)
}

/// The `pnpm:global` warn messages [`RecordingReporter`] captured.
#[must_use]
pub fn warns() -> Vec<String> {
    messages_at(LogLevel::Warn)
}

/// The captured `pnpm:global` messages at `level`.
#[must_use]
pub fn messages_at(level: LogLevel) -> Vec<String> {
    EMITTED.with(|emitted| {
        emitted
            .borrow()
            .iter()
            .filter(|(emitted_level, _)| *emitted_level == level)
            .map(|(_, message)| message.clone())
            .collect()
    })
}

/// A still-pending web-auth poll response (HTTP 202, keep polling).
#[must_use]
pub fn ok_202() -> WebAuthFetchResponse {
    WebAuthFetchResponse { ok: true, status: 202, retry_after: None, body: "{}".to_owned() }
}

/// A completed web-auth poll response carrying the granted `token`.
#[must_use]
pub fn ok_token(token: &str) -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: true,
        status: 200,
        retry_after: None,
        body: json!({ "token": token }).to_string(),
    }
}

/// The `authUrl` / `doneUrl` pair a web-auth OTP challenge body carries.
#[must_use]
pub fn web_auth_body() -> Option<OtpErrorBody> {
    Some(OtpErrorBody {
        auth_url: Some("https://registry.npmjs.org/auth/abc".to_owned()),
        done_url: Some("https://registry.npmjs.org/auth/abc/done".to_owned()),
    })
}
