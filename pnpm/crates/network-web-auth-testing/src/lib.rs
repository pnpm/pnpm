//! Shared test fakes for the OTP / web-auth flow
//! ([`pacquet_network_web_auth`]).
//!
//! The OTP / web-auth tests need a fake for every web-auth capability. This
//! crate keeps the fake's mutable pieces per-test: the [`web_auth_fake`] macro
//! expands, inside a `#[test]` body, to fn-local `thread_local!` statics plus
//! a local `FakeHost` (implementing every web-auth capability), local
//! reporters, and local config functions. No scenario state lives at module
//! scope, so concurrently running tests can never share or race on it — this
//! is the "state in a `static` inside the `#[test]` body" rule of the
//! "Dependency injection for tests" section of `pnpm/CODE_STYLE_GUIDE.md`.
//!
//! The stateless pieces — [`InputResponse`], [`SleepBehavior`],
//! [`FetchScript`], [`FakeOtpError`], and the response builders
//! [`ok_202`], [`ok_token`], [`web_auth_body`] — carry no mutable state, so
//! they stay ordinary `pub` items shared across every test.

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_network_web_auth::{
    OtpChallenge, OtpError, OtpErrorBody, WebAuthFetchError, WebAuthFetchResponse,
};
use serde_json::{json, to_vec};

/// An operation error that is either an EOTP challenge or a plain failure, so
/// a single error type covers both the OTP and non-OTP paths a fake operation
/// needs to return.
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

/// What the `PromptOtp` fake returns for the classic-OTP prompt.
pub enum InputResponse {
    Value(Option<String>),
    Cancelled,
}

/// How the `Sleep` fake advances the fake clock when awaited — left alone,
/// or jumped forward by a fixed number of milliseconds to drive the web-auth
/// poll past its deadline.
#[derive(Clone, Copy)]
pub enum SleepBehavior {
    NoAdvance,
    AdvanceByFixed(u64),
}

/// A scripted sequence of `WebAuthFetch` responses: each poll calls it once.
pub type FetchScript = Box<dyn FnMut() -> Result<WebAuthFetchResponse, WebAuthFetchError>>;

/// Expand a per-test web-auth fake at the top of a `#[test]` (or
/// `#[tokio::test]`) body.
///
/// Invoked as `web_auth_fake!();`, it declares — as items local to the test
/// function — a `thread_local!` block of scenario statics, a unit `FakeHost`
/// implementing all eight web-auth capability traits over those statics, the
/// `RecordingReporter` / `UnexpectedReporter` sinks, and the `reset` /
/// `set_*` / `infos` / `warns` / `messages_at` config functions. Because the
/// statics live inside the test function, every test gets its own storage;
/// nothing is shared at module scope, so concurrently running tests can
/// never race on the scenario. This is the "state in a `static` inside the
/// `#[test]` body" rule of the "Dependency injection for tests" section of
/// `pnpm/CODE_STYLE_GUIDE.md`.
///
/// The generated items reference this crate's stateless helpers —
/// [`InputResponse`], [`SleepBehavior`], [`FetchScript`] — through `$crate`,
/// and everything else through absolute paths, so a caller needs only to
/// import the macro, not any of the items it names.
///
/// The generated `FakeHost` and `set_*` / query helpers carry
/// `#[allow(dead_code)]`: the macro emits the complete fake surface into every
/// test, but each test drives only the capabilities its scenario needs, so the
/// unused ones are expected rather than a lint to fix.
#[macro_export]
macro_rules! web_auth_fake {
    () => {
        ::std::thread_local! {
            static STDIN_TTY: ::std::cell::Cell<bool> = const { ::std::cell::Cell::new(true) };
            static STDOUT_TTY: ::std::cell::Cell<bool> = const { ::std::cell::Cell::new(true) };
            static TIME: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(0) };
            static SLEEP_BEHAVIOR: ::std::cell::Cell<$crate::SleepBehavior> =
                const { ::std::cell::Cell::new($crate::SleepBehavior::NoAdvance) };
            static FETCH: ::std::cell::RefCell<::std::option::Option<$crate::FetchScript>> =
                const { ::std::cell::RefCell::new(::std::option::Option::None) };
            static INPUT: ::std::cell::RefCell<$crate::InputResponse> = const {
                ::std::cell::RefCell::new($crate::InputResponse::Value(::std::option::Option::None))
            };
            static ENTER_TX: ::std::cell::RefCell<
                ::std::option::Option<::tokio::sync::oneshot::Sender<()>>,
            > = const { ::std::cell::RefCell::new(::std::option::Option::None) };
            static EMITTED: ::std::cell::RefCell<
                ::std::vec::Vec<(::pacquet_reporter::LogLevel, ::std::string::String)>,
            > = const { ::std::cell::RefCell::new(::std::vec::Vec::new()) };
        }

        /// The fake web-auth host: every capability reads from the fn-local
        /// thread-local script the `set_*` functions configure.
        #[allow(dead_code)]
        struct FakeHost;

        impl ::pacquet_network_web_auth::StdinIsTty for FakeHost {
            fn stdin_is_tty() -> bool {
                STDIN_TTY.with(::std::cell::Cell::get)
            }
        }

        impl ::pacquet_network_web_auth::StdoutIsTty for FakeHost {
            fn stdout_is_tty() -> bool {
                STDOUT_TTY.with(::std::cell::Cell::get)
            }
        }

        impl ::pacquet_network_web_auth::Clock for FakeHost {
            fn now_ms() -> u64 {
                TIME.with(::std::cell::Cell::get)
            }
        }

        impl ::pacquet_network_web_auth::Sleep for FakeHost {
            fn sleep_ms(ms: u64) -> impl ::std::future::Future<Output = ()> {
                let _ = ms;
                if let $crate::SleepBehavior::AdvanceByFixed(jump) =
                    SLEEP_BEHAVIOR.with(::std::cell::Cell::get)
                {
                    TIME.with(|time| time.set(time.get().saturating_add(jump)));
                }
                ::std::future::ready(())
            }
        }

        impl ::pacquet_network_web_auth::WebAuthFetch for FakeHost {
            fn fetch(
                _url: &str,
                _options: &::pacquet_network_web_auth::WebAuthFetchOptions,
            ) -> impl ::std::future::Future<
                Output = ::std::result::Result<
                    ::pacquet_network_web_auth::WebAuthFetchResponse,
                    ::pacquet_network_web_auth::WebAuthFetchError,
                >,
            > {
                let result = FETCH.with(|fetch| {
                    (fetch.borrow_mut().as_mut().expect("a fetch script must be set"))()
                });
                ::std::future::ready(result)
            }
        }

        impl ::pacquet_network_web_auth::PromptOtp for FakeHost {
            fn input(
                _message: &str,
            ) -> impl ::std::future::Future<
                Output = ::std::result::Result<
                    ::std::option::Option<::std::string::String>,
                    ::pacquet_network_web_auth::PromptError,
                >,
            > {
                let response = INPUT.with(|input| match &*input.borrow() {
                    $crate::InputResponse::Value(value) => ::std::result::Result::Ok(value.clone()),
                    $crate::InputResponse::Cancelled => ::std::result::Result::Err(
                        ::pacquet_network_web_auth::PromptError::Cancelled,
                    ),
                });
                ::std::future::ready(response)
            }
        }

        impl ::pacquet_network_web_auth::OpenUrl for FakeHost {
            fn open_url(_url: &str) -> ::std::io::Result<()> {
                ::std::result::Result::Ok(())
            }
        }

        /// Never resolves on its own — in these tests the web-auth poll always
        /// wins or times out before any Enter keypress.
        #[allow(dead_code)]
        struct PendingEnterHandle {
            rx: ::tokio::sync::oneshot::Receiver<()>,
        }

        impl ::std::future::Future for PendingEnterHandle {
            type Output = ();

            fn poll(
                self: ::std::pin::Pin<&mut Self>,
                cx: &mut ::std::task::Context<'_>,
            ) -> ::std::task::Poll<()> {
                ::std::future::Future::poll(::std::pin::Pin::new(&mut self.get_mut().rx), cx)
                    .map(|_| ())
            }
        }

        impl ::pacquet_network_web_auth::EnterKeyListener for FakeHost {
            type Handle = PendingEnterHandle;

            fn listen() -> ::std::io::Result<PendingEnterHandle> {
                let (tx, rx) = ::tokio::sync::oneshot::channel();
                ENTER_TX.with(|cell| *cell.borrow_mut() = ::std::option::Option::Some(tx));
                ::std::result::Result::Ok(PendingEnterHandle { rx })
            }
        }

        /// Records every `pnpm:global` message so a test can assert on the
        /// auth URL / warnings the flow surfaces.
        #[allow(dead_code)]
        struct RecordingReporter;

        impl ::pacquet_reporter::Reporter for RecordingReporter {
            fn emit(event: &::pacquet_reporter::LogEvent) {
                if let ::pacquet_reporter::LogEvent::Global(::pacquet_reporter::GlobalLog {
                    level,
                    message,
                }) = event
                {
                    EMITTED.with(|emitted| emitted.borrow_mut().push((*level, message.clone())));
                }
            }
        }

        /// Panics on any global message — the strict reporter for a test that
        /// expects no warning.
        #[allow(dead_code)]
        struct UnexpectedReporter;

        impl ::pacquet_reporter::Reporter for UnexpectedReporter {
            fn emit(event: &::pacquet_reporter::LogEvent) {
                if let ::pacquet_reporter::LogEvent::Global(::pacquet_reporter::GlobalLog {
                    message,
                    ..
                }) = event
                {
                    panic!("unexpected global message: {message}");
                }
            }
        }

        /// Clear every thread-local script back to its default.
        #[allow(dead_code)]
        fn reset() {
            STDIN_TTY.with(|tty| tty.set(true));
            STDOUT_TTY.with(|tty| tty.set(true));
            TIME.with(|time| time.set(0));
            SLEEP_BEHAVIOR.with(|behavior| behavior.set($crate::SleepBehavior::NoAdvance));
            FETCH.with(|fetch| *fetch.borrow_mut() = ::std::option::Option::None);
            INPUT.with(|input| {
                *input.borrow_mut() = $crate::InputResponse::Value(::std::option::Option::None);
            });
            ENTER_TX.with(|cell| *cell.borrow_mut() = ::std::option::Option::None);
            EMITTED.with(|emitted| emitted.borrow_mut().clear());
        }

        /// Whether `FakeHost` reports stdin as a TTY (drives the
        /// interactive-prompt gate).
        #[allow(dead_code)]
        fn set_stdin_tty(is_tty: bool) {
            STDIN_TTY.with(|tty| tty.set(is_tty));
        }

        /// Whether `FakeHost` reports stdout as a TTY.
        #[allow(dead_code)]
        fn set_stdout_tty(is_tty: bool) {
            STDOUT_TTY.with(|tty| tty.set(is_tty));
        }

        /// Set the fake clock (milliseconds) `FakeHost`'s clock reads.
        #[allow(dead_code)]
        fn set_time(ms: u64) {
            TIME.with(|time| time.set(ms));
        }

        /// Choose how `FakeHost`'s sleep advances the fake clock.
        #[allow(dead_code)]
        fn set_sleep_behavior(behavior: $crate::SleepBehavior) {
            SLEEP_BEHAVIOR.with(|cell| cell.set(behavior));
        }

        /// Script what the classic-OTP prompt returns.
        #[allow(dead_code)]
        fn set_input(response: $crate::InputResponse) {
            INPUT.with(|input| *input.borrow_mut() = response);
        }

        /// Script the web-auth poll responses.
        #[allow(dead_code)]
        fn set_fetch(script: $crate::FetchScript) {
            FETCH.with(|fetch| *fetch.borrow_mut() = ::std::option::Option::Some(script));
        }

        /// The `pnpm:global` info messages `RecordingReporter` captured.
        #[allow(dead_code)]
        fn infos() -> ::std::vec::Vec<::std::string::String> {
            messages_at(::pacquet_reporter::LogLevel::Info)
        }

        /// The `pnpm:global` warn messages `RecordingReporter` captured.
        #[allow(dead_code)]
        fn warns() -> ::std::vec::Vec<::std::string::String> {
            messages_at(::pacquet_reporter::LogLevel::Warn)
        }

        /// The captured `pnpm:global` messages at `level`.
        #[allow(dead_code)]
        fn messages_at(
            level: ::pacquet_reporter::LogLevel,
        ) -> ::std::vec::Vec<::std::string::String> {
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

/// A still-pending web-auth poll response (HTTP 202, keep polling).
#[must_use]
pub fn ok_202() -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: true,
        status: 202,
        retry_after: None,
        body: b"{}".to_vec(),
        truncated: false,
    }
}

/// A completed web-auth poll response carrying the granted `token`.
#[must_use]
pub fn ok_token(token: &str) -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: true,
        status: 200,
        retry_after: None,
        body: to_vec(&json!({ "token": token })).unwrap(),
        truncated: false,
    }
}

/// A completed web-auth poll response whose body the provider capped at the
/// size limit (`truncated`), simulating a registry that returned an
/// over-cap body. Drives the token-body-limit branch through the
/// dependency-injection seam.
#[must_use]
pub fn ok_truncated() -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: true,
        status: 200,
        retry_after: None,
        // A real token, to prove that a truncated response is discarded
        // *because* it was truncated, not because the body lacked a token.
        body: to_vec(&json!({ "token": "web-token-123" })).unwrap(),
        truncated: true,
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
