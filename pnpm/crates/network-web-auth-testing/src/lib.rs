//! Shared test fakes for the OTP / web-auth flow
//! ([`pnpm_network_web_auth`]).
//!
//! The OTP / web-auth tests need a fake for every web-auth capability. This
//! crate keeps the fake's mutable pieces per-test: the [`web_auth_fake`] macro
//! expands, inside a `#[test]` body, to fn-local `thread_local!` statics and a
//! `reset`, plus — one per named argument — a `FakeHost` (implementing every
//! web-auth capability), the recording / strict reporters, and the `set_*` /
//! `infos` / `warns` config functions. No scenario state lives at module
//! scope, so concurrently running tests can never share or race on it — this
//! is the "state in a `static` inside the `#[test]` body" rule of the
//! "Dependency injection for tests" section of `pnpm/CODE_STYLE_GUIDE.md`.
//!
//! The stateless pieces — [`InputResponse`], [`SleepBehavior`],
//! [`FetchScript`], [`FakeOtpError`], and the response builders
//! [`ok_202`], [`ok_token`], [`web_auth_body`] — carry no mutable state, so
//! they stay ordinary `pub` items shared across every test.

use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_network_web_auth::{
    OtpChallenge, OtpError, OtpErrorBody, WebAuthFetchError, WebAuthFetchResponse,
};
use serde_json::json;

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
/// The always-emitted core is the `thread_local!` block of scenario statics
/// and `reset`, which clears every static back to its default. Call
/// `reset()` first in each test: because the statics live inside the test
/// function, every test gets its own storage and concurrently running tests
/// never race on the scenario — the "state in a `static` inside the `#[test]`
/// body" rule of the "Dependency injection for tests" section of
/// `pnpm/CODE_STYLE_GUIDE.md`.
///
/// Everything a test actually drives is generated from a named argument, so no
/// helper is emitted unused and none needs an `#[allow(dead_code)]`. Name
/// exactly the ones the scenario uses:
///
/// - `FakeHost` — the unit host implementing all eight web-auth capabilities
///   over the statics, i.e. the `Sys` provider the flow runs against.
/// - `RecordingReporter` — captures every `pnpm:global` message for `infos` /
///   `warns` to read; `UnexpectedReporter` — panics on any emission, for a
///   scenario that expects none.
/// - `set_stdin_tty` / `set_stdout_tty` / `set_time` / `set_sleep_behavior` /
///   `set_input` / `set_fetch` — script one capability's behavior.
/// - `infos` / `warns` — the captured `pnpm:global` messages at that level.
///
/// A mistyped helper name is rejected with a `compile_error!` listing the
/// valid names.
///
/// The generated items reference this crate's stateless helpers —
/// [`InputResponse`], [`SleepBehavior`], [`FetchScript`] — through `$crate`,
/// and everything else through absolute paths, so a caller needs only to
/// import the macro, not any of the items it names.
#[macro_export]
macro_rules! web_auth_fake {
    ($($helper:ident),* $(,)?) => {
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
                ::std::vec::Vec<(::pnpm_reporter::LogLevel, ::std::string::String)>,
            > = const { ::std::cell::RefCell::new(::std::vec::Vec::new()) };
        }

        /// Clear every thread-local script back to its default. Called first
        /// in each test, which is also what keeps every static exercised.
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

        $( $crate::web_auth_fake!(@helper $helper); )*
    };

    (@helper FakeHost) => {
        /// The fake web-auth host: every capability reads from the fn-local
        /// thread-local script the `set_*` functions configure.
        struct FakeHost;

        impl ::pnpm_network_web_auth::StdinIsTty for FakeHost {
            fn stdin_is_tty() -> bool {
                STDIN_TTY.with(::std::cell::Cell::get)
            }
        }

        impl ::pnpm_network_web_auth::StdoutIsTty for FakeHost {
            fn stdout_is_tty() -> bool {
                STDOUT_TTY.with(::std::cell::Cell::get)
            }
        }

        impl ::pnpm_network_web_auth::Clock for FakeHost {
            fn now_ms() -> u64 {
                TIME.with(::std::cell::Cell::get)
            }
        }

        impl ::pnpm_network_web_auth::Sleep for FakeHost {
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

        impl ::pnpm_network_web_auth::WebAuthFetch for FakeHost {
            fn fetch(
                _url: &str,
                _options: &::pnpm_network_web_auth::WebAuthFetchOptions,
            ) -> impl ::std::future::Future<
                Output = ::std::result::Result<
                    ::pnpm_network_web_auth::WebAuthFetchResponse,
                    ::pnpm_network_web_auth::WebAuthFetchError,
                >,
            > {
                let result = FETCH.with(|fetch| {
                    (fetch.borrow_mut().as_mut().expect("a fetch script must be set"))()
                });
                ::std::future::ready(result)
            }
        }

        impl ::pnpm_network_web_auth::PromptOtp for FakeHost {
            fn input(
                _message: &str,
            ) -> impl ::std::future::Future<
                Output = ::std::result::Result<
                    ::std::option::Option<::std::string::String>,
                    ::pnpm_network_web_auth::PromptError,
                >,
            > {
                let response = INPUT.with(|input| match &*input.borrow() {
                    $crate::InputResponse::Value(value) => ::std::result::Result::Ok(value.clone()),
                    $crate::InputResponse::Cancelled => ::std::result::Result::Err(
                        ::pnpm_network_web_auth::PromptError::Cancelled,
                    ),
                });
                ::std::future::ready(response)
            }
        }

        impl ::pnpm_network_web_auth::OpenUrl for FakeHost {
            fn open_url(_url: &str) -> ::std::io::Result<()> {
                ::std::result::Result::Ok(())
            }
        }

        /// Never resolves on its own — in these tests the web-auth poll always
        /// wins or times out before any Enter keypress.
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

        impl ::pnpm_network_web_auth::EnterKeyListener for FakeHost {
            type Handle = PendingEnterHandle;

            fn listen() -> ::std::io::Result<PendingEnterHandle> {
                let (tx, rx) = ::tokio::sync::oneshot::channel();
                ENTER_TX.with(|cell| *cell.borrow_mut() = ::std::option::Option::Some(tx));
                ::std::result::Result::Ok(PendingEnterHandle { rx })
            }
        }
    };

    (@helper RecordingReporter) => {
        /// Records every `pnpm:global` message so a test can assert on the
        /// auth URL / warnings the flow surfaces.
        struct RecordingReporter;

        impl ::pnpm_reporter::Reporter for RecordingReporter {
            fn emit(event: &::pnpm_reporter::LogEvent) {
                if let ::pnpm_reporter::LogEvent::Global(::pnpm_reporter::GlobalLog {
                    level,
                    message,
                }) = event
                {
                    EMITTED.with(|emitted| emitted.borrow_mut().push((*level, message.clone())));
                }
            }
        }
    };

    (@helper UnexpectedReporter) => {
        /// Panics on any log event — the strict reporter for a test that
        /// expects no emission at all.
        struct UnexpectedReporter;

        impl ::pnpm_reporter::Reporter for UnexpectedReporter {
            fn emit(event: &::pnpm_reporter::LogEvent) {
                panic!("unexpected log: {event:?}");
            }
        }
    };

    (@helper set_stdin_tty) => {
        /// Whether `FakeHost` reports stdin as a TTY (drives the
        /// interactive-prompt gate).
        fn set_stdin_tty(is_tty: bool) {
            STDIN_TTY.with(|tty| tty.set(is_tty));
        }
    };

    (@helper set_stdout_tty) => {
        /// Whether `FakeHost` reports stdout as a TTY.
        fn set_stdout_tty(is_tty: bool) {
            STDOUT_TTY.with(|tty| tty.set(is_tty));
        }
    };

    (@helper set_time) => {
        /// Set the fake clock (milliseconds) `FakeHost`'s clock reads.
        fn set_time(ms: u64) {
            TIME.with(|time| time.set(ms));
        }
    };

    (@helper set_sleep_behavior) => {
        /// Choose how `FakeHost`'s sleep advances the fake clock.
        fn set_sleep_behavior(behavior: $crate::SleepBehavior) {
            SLEEP_BEHAVIOR.with(|cell| cell.set(behavior));
        }
    };

    (@helper set_input) => {
        /// Script what the classic-OTP prompt returns.
        fn set_input(response: $crate::InputResponse) {
            INPUT.with(|input| *input.borrow_mut() = response);
        }
    };

    (@helper set_fetch) => {
        /// Script the web-auth poll responses.
        fn set_fetch(script: $crate::FetchScript) {
            FETCH.with(|fetch| *fetch.borrow_mut() = ::std::option::Option::Some(script));
        }
    };

    (@helper infos) => {
        /// The `pnpm:global` info messages `RecordingReporter` captured.
        fn infos() -> ::std::vec::Vec<::std::string::String> {
            EMITTED.with(|emitted| {
                emitted
                    .borrow()
                    .iter()
                    .filter(|(level, _)| *level == ::pnpm_reporter::LogLevel::Info)
                    .map(|(_, message)| message.clone())
                    .collect()
            })
        }
    };

    (@helper warns) => {
        /// The `pnpm:global` warn messages `RecordingReporter` captured.
        fn warns() -> ::std::vec::Vec<::std::string::String> {
            EMITTED.with(|emitted| {
                emitted
                    .borrow()
                    .iter()
                    .filter(|(level, _)| *level == ::pnpm_reporter::LogLevel::Warn)
                    .map(|(_, message)| message.clone())
                    .collect()
            })
        }
    };

    (@helper $unknown:ident) => {
        ::std::compile_error!(::std::concat!(
            "unknown `web_auth_fake!` helper `",
            ::std::stringify!($unknown),
            "`; expected one of: FakeHost, RecordingReporter, UnexpectedReporter, set_stdin_tty, ",
            "set_stdout_tty, set_time, set_sleep_behavior, set_input, set_fetch, infos, warns",
        ));
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
        body: json!({ "token": token }).to_string().into_bytes(),
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
        body: json!({ "token": "web-token-123" }).to_string().into_bytes(),
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
