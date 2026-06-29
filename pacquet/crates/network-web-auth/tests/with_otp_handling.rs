use std::{
    cell::{Cell, RefCell},
    future::{self, Future},
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::oneshot;

use super::{
    OtpChallenge, OtpError, OtpErrorBody, SyntheticOtpError, WithOtpError, with_otp_handling,
};
use crate::{
    capabilities::{
        Clock, EnterKeyListener, OpenUrl, PromptError, PromptOtp, Sleep, StdinIsTty, StdoutIsTty,
        WebAuthFetch, WebAuthFetchError,
    },
    poll_for_web_auth_token::{WebAuthFetchOptions, WebAuthFetchResponse},
};

/// An operation error that is either an EOTP challenge or a plain failure,
/// so a single `Error` type covers both the OTP and non-OTP test paths.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
enum TestError {
    #[display("otp challenge")]
    Otp { body: Option<OtpErrorBody> },
    #[display("{_0}")]
    Other(#[error(not(source))] String),
}

impl OtpError for TestError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        match self {
            TestError::Otp { body } => Some(OtpChallenge { body: body.clone() }),
            TestError::Other(_) => None,
        }
    }
}

/// What the [`PromptOtp`] fake returns for the classic-OTP prompt.
enum InputResponse {
    Value(Option<String>),
    Cancelled,
}

#[derive(Clone, Copy)]
enum SleepBehavior {
    NoAdvance,
    AdvanceByFixed(u64),
}

type FetchScript = Box<dyn FnMut() -> Result<WebAuthFetchResponse, WebAuthFetchError>>;

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

struct Fake;

impl StdinIsTty for Fake {
    fn stdin_is_tty() -> bool {
        STDIN_TTY.with(Cell::get)
    }
}

impl StdoutIsTty for Fake {
    fn stdout_is_tty() -> bool {
        STDOUT_TTY.with(Cell::get)
    }
}

impl Clock for Fake {
    fn now_ms() -> u64 {
        TIME.with(Cell::get)
    }
}

impl Sleep for Fake {
    fn sleep_ms(ms: u64) -> impl Future<Output = ()> {
        let _ = ms;
        if let SleepBehavior::AdvanceByFixed(jump) = SLEEP_BEHAVIOR.with(Cell::get) {
            TIME.with(|time| time.set(time.get().saturating_add(jump)));
        }
        future::ready(())
    }
}

impl WebAuthFetch for Fake {
    fn fetch(
        _url: &str,
        _options: &WebAuthFetchOptions,
    ) -> impl Future<Output = Result<WebAuthFetchResponse, WebAuthFetchError>> {
        let result = FETCH
            .with(|fetch| (fetch.borrow_mut().as_mut().expect("a fetch script must be set"))());
        future::ready(result)
    }
}

impl PromptOtp for Fake {
    fn input(_message: &str) -> impl Future<Output = Result<Option<String>, PromptError>> {
        let response = INPUT.with(|input| match &*input.borrow() {
            InputResponse::Value(value) => Ok(value.clone()),
            InputResponse::Cancelled => Err(PromptError::Cancelled),
        });
        future::ready(response)
    }
}

impl OpenUrl for Fake {
    fn open_url(_url: &str) -> io::Result<()> {
        Ok(())
    }
}

/// Never resolves on its own — in these tests the web-auth poll always
/// wins or times out before any Enter keypress.
struct PendingEnterHandle {
    rx: oneshot::Receiver<()>,
}

impl Future for PendingEnterHandle {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        Pin::new(&mut self.get_mut().rx).poll(cx).map(|_| ())
    }
}

impl EnterKeyListener for Fake {
    type Handle = PendingEnterHandle;

    fn listen() -> io::Result<PendingEnterHandle> {
        let (tx, rx) = oneshot::channel();
        ENTER_TX.with(|cell| *cell.borrow_mut() = Some(tx));
        Ok(PendingEnterHandle { rx })
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

/// Panics on any global message — the stand-in for the TS `globalWarn`
/// that throws when a test expects no warning.
struct UnexpectedReporter;

impl Reporter for UnexpectedReporter {
    fn emit(event: &LogEvent) {
        if let LogEvent::Global(GlobalLog { message, .. }) = event {
            panic!("unexpected global message: {message}");
        }
    }
}

fn reset() {
    STDIN_TTY.with(|tty| tty.set(true));
    STDOUT_TTY.with(|tty| tty.set(true));
    TIME.with(|time| time.set(0));
    SLEEP_BEHAVIOR.with(|behavior| behavior.set(SleepBehavior::NoAdvance));
    FETCH.with(|fetch| *fetch.borrow_mut() = None);
    INPUT.with(|input| *input.borrow_mut() = InputResponse::Value(None));
    ENTER_TX.with(|cell| *cell.borrow_mut() = None);
    EMITTED.with(|emitted| emitted.borrow_mut().clear());
}

fn set_input(response: InputResponse) {
    INPUT.with(|input| *input.borrow_mut() = response);
}

fn set_fetch(script: FetchScript) {
    FETCH.with(|fetch| *fetch.borrow_mut() = Some(script));
}

fn infos() -> Vec<String> {
    messages_at(LogLevel::Info)
}

fn warns() -> Vec<String> {
    messages_at(LogLevel::Warn)
}

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

fn ok_202() -> WebAuthFetchResponse {
    WebAuthFetchResponse { ok: true, status: 202, retry_after: None, body: "{}".to_owned() }
}

fn ok_token(token: &str) -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: true,
        status: 200,
        retry_after: None,
        body: json!({ "token": token }).to_string(),
    }
}

fn web_auth_body() -> Option<OtpErrorBody> {
    Some(OtpErrorBody {
        auth_url: Some("https://registry.npmjs.org/auth/abc".to_owned()),
        done_url: Some("https://registry.npmjs.org/auth/abc/done".to_owned()),
    })
}

#[tokio::test]
async fn returns_the_result_when_the_operation_succeeds_without_otp() {
    reset();

    let result = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async |_otp| Ok("success".to_owned()),
    )
    .await
    .expect("a result");

    assert_eq!(result, "success");
}

#[tokio::test]
async fn throws_non_otp_errors_as_is() {
    reset();

    let error = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async |_otp| Err(TestError::Other("network error".to_owned())),
    )
    .await
    .expect_err("an error");

    assert!(
        matches!(&error, WithOtpError::Operation(TestError::Other(message)) if message == "network error"),
        "expected the original error, got {error:?}",
    );
}

#[tokio::test]
async fn throws_non_interactive_error_when_stdin_is_not_interactive() {
    reset();
    STDIN_TTY.with(|tty| tty.set(false));

    let error = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async |_otp| Err(TestError::Otp { body: None }),
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::NonInteractive(_)), "got {error:?}");
}

#[tokio::test]
async fn throws_non_interactive_error_when_stdout_is_not_interactive() {
    reset();
    STDOUT_TTY.with(|tty| tty.set(false));

    let error = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async |_otp| Err(TestError::Otp { body: None }),
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::NonInteractive(_)), "got {error:?}");
}

#[tokio::test]
async fn classic_flow_prompts_for_otp_and_retries_operation() {
    reset();
    set_input(InputResponse::Value(Some("654321".to_owned())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let result = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async move |otp| {
            counter.set(counter.get() + 1);
            if counter.get() == 1 {
                Err(TestError::Otp { body: None })
            } else {
                assert_eq!(otp, Some("654321"));
                Ok("ok".to_owned())
            }
        },
    )
    .await
    .expect("a result");

    assert_eq!(result, "ok");
    assert_eq!(calls.get(), 2);
}

#[tokio::test]
async fn classic_flow_throws_second_challenge_error_if_retry_also_requires_otp() {
    reset();
    set_input(InputResponse::Value(Some("123456".to_owned())));

    let error = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async |_otp| Err(TestError::Otp { body: None }),
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::SecondChallenge(_)), "got {error:?}");
}

#[tokio::test]
async fn classic_flow_throws_non_otp_errors_from_the_retry_as_is() {
    reset();
    set_input(InputResponse::Value(Some("123456".to_owned())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let error = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async move |_otp| {
            counter.set(counter.get() + 1);
            if counter.get() == 1 {
                Err(TestError::Otp { body: None })
            } else {
                Err(TestError::Other("server error".to_owned()))
            }
        },
    )
    .await
    .expect_err("an error");

    assert!(
        matches!(&error, WithOtpError::Operation(TestError::Other(message)) if message == "server error"),
        "got {error:?}",
    );
}

#[tokio::test]
async fn classic_flow_re_throws_the_original_otp_error_when_prompt_returns_empty() {
    reset();
    set_input(InputResponse::Value(Some(String::new())));

    let error = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async |_otp| Err(TestError::Otp { body: None }),
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::Operation(TestError::Otp { .. })), "got {error:?}");
}

#[tokio::test]
async fn classic_flow_re_throws_the_original_otp_error_when_prompt_returns_none() {
    reset();
    set_input(InputResponse::Value(None));

    let error = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async |_otp| Err(TestError::Otp { body: None }),
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::Operation(TestError::Otp { .. })), "got {error:?}");
}

#[tokio::test]
async fn classic_flow_re_throws_the_original_otp_error_when_prompt_is_cancelled() {
    reset();
    set_input(InputResponse::Cancelled);

    let error = with_otp_handling::<Fake, UnexpectedReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async |_otp| Err(TestError::Otp { body: None }),
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::Operation(TestError::Otp { .. })), "got {error:?}");
}

/// Unlike the TS test, which omits `createReadlineInterface` so the prompt
/// stays silent, pacquet's listener is always available — so the flow also
/// emits the "Press ENTER" line. The assertion checks that the auth URL
/// was surfaced and the token round-tripped, not the exact message count.
#[tokio::test]
async fn web_auth_flow_polls_done_url_and_uses_returned_token() {
    reset();
    let fetch_calls = Rc::new(Cell::new(0));
    let fetch_counter = Rc::clone(&fetch_calls);
    set_fetch(Box::new(move || {
        fetch_counter.set(fetch_counter.get() + 1);
        Ok(if fetch_counter.get() < 3 { ok_202() } else { ok_token("web-token-123") })
    }));
    let op_calls = Rc::new(Cell::new(0));
    let op_counter = Rc::clone(&op_calls);

    let result = with_otp_handling::<Fake, RecordingReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async move |otp| {
            op_counter.set(op_counter.get() + 1);
            if op_counter.get() == 1 {
                Err(TestError::Otp { body: web_auth_body() })
            } else {
                assert_eq!(otp, Some("web-token-123"));
                Ok("published".to_owned())
            }
        },
    )
    .await
    .expect("a result");

    assert_eq!(result, "published");
    assert_eq!(op_calls.get(), 2);
    assert_eq!(fetch_calls.get(), 3);
    assert!(
        infos().iter().any(|message| message.contains("https://registry.npmjs.org/auth/abc")),
        "the auth URL should be surfaced, got {:?}",
        infos(),
    );
}

#[tokio::test]
async fn web_auth_flow_falls_back_to_classic_prompt_when_only_auth_url_is_present() {
    reset();
    set_input(InputResponse::Value(Some("manual-code".to_owned())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let result = with_otp_handling::<Fake, RecordingReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async move |otp| {
            counter.set(counter.get() + 1);
            if counter.get() == 1 {
                Err(TestError::Otp {
                    body: Some(OtpErrorBody {
                        auth_url: Some("https://registry.npmjs.org/auth/abc".to_owned()),
                        done_url: None,
                    }),
                })
            } else {
                assert_eq!(otp, Some("manual-code"));
                Ok("done".to_owned())
            }
        },
    )
    .await
    .expect("a result");

    assert_eq!(result, "done");
}

#[tokio::test]
async fn web_auth_flow_falls_back_to_classic_prompt_when_only_done_url_is_present() {
    reset();
    set_input(InputResponse::Value(Some("manual-code".to_owned())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let result = with_otp_handling::<Fake, RecordingReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async move |otp| {
            counter.set(counter.get() + 1);
            if counter.get() == 1 {
                Err(TestError::Otp {
                    body: Some(OtpErrorBody {
                        auth_url: None,
                        done_url: Some("https://registry.npmjs.org/auth/abc/done".to_owned()),
                    }),
                })
            } else {
                assert_eq!(otp, Some("manual-code"));
                Ok("done".to_owned())
            }
        },
    )
    .await
    .expect("a result");

    assert_eq!(result, "done");
}

#[tokio::test]
async fn web_auth_flow_throws_timeout_error_when_polling_times_out() {
    reset();
    SLEEP_BEHAVIOR.with(|behavior| behavior.set(SleepBehavior::AdvanceByFixed(6 * 60 * 1000)));
    set_fetch(Box::new(|| Ok(ok_202())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let error = with_otp_handling::<Fake, RecordingReporter, String, TestError, _>(
        WebAuthFetchOptions::default(),
        async move |_otp| {
            counter.set(counter.get() + 1);
            assert_eq!(counter.get(), 1, "the operation must not be retried after a timeout");
            Err(TestError::Otp { body: web_auth_body() })
        },
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::Timeout(_)), "got {error:?}");
    assert!(
        infos().iter().any(|message| message.contains("https://registry.npmjs.org/auth/abc")),
        "the auth URL should be surfaced, got {:?}",
        infos(),
    );
}

#[test]
fn synthetic_otp_error_is_an_otp_challenge() {
    let error = SyntheticOtpError::new(web_auth_body());
    assert!(error.as_otp_challenge().is_some());
}

#[test]
fn synthetic_otp_error_stores_body() {
    let body = OtpErrorBody {
        auth_url: Some("https://example.com/auth".to_owned()),
        done_url: Some("https://example.com/done".to_owned()),
    };
    let error = SyntheticOtpError::new(Some(body.clone()));
    assert_eq!(error.as_otp_challenge().expect("a challenge").body, Some(body));
}

#[test]
fn from_unknown_body_extracts_valid_string_auth_url_and_done_url() {
    let error = SyntheticOtpError::from_unknown_body::<UnexpectedReporter>(Some(
        &json!({ "authUrl": "https://example.com/auth", "doneUrl": "https://example.com/done" }),
    ));
    assert_eq!(
        error.as_otp_challenge().expect("a challenge").body,
        Some(OtpErrorBody {
            auth_url: Some("https://example.com/auth".to_owned()),
            done_url: Some("https://example.com/done".to_owned()),
        }),
    );
}

#[test]
fn from_unknown_body_returns_no_body_when_body_is_null() {
    let error = SyntheticOtpError::from_unknown_body::<UnexpectedReporter>(Some(&json!(null)));
    assert_eq!(error.as_otp_challenge().expect("a challenge").body, None);
}

#[test]
fn from_unknown_body_returns_no_body_when_body_is_not_an_object() {
    let error =
        SyntheticOtpError::from_unknown_body::<UnexpectedReporter>(Some(&json!("not an object")));
    assert_eq!(error.as_otp_challenge().expect("a challenge").body, None);
}

#[test]
fn from_unknown_body_warns_when_auth_url_has_wrong_type() {
    reset();
    let error = SyntheticOtpError::from_unknown_body::<RecordingReporter>(Some(
        &json!({ "authUrl": 123, "doneUrl": "https://example.com/done" }),
    ));
    assert!(warns().iter().any(|message| message.contains("authUrl")), "got {:?}", warns());
    let body = error.as_otp_challenge().expect("a challenge").body.expect("a body");
    assert_eq!(body.auth_url, None);
    assert_eq!(body.done_url, Some("https://example.com/done".to_owned()));
}

#[test]
fn from_unknown_body_warns_when_done_url_has_wrong_type() {
    reset();
    let error = SyntheticOtpError::from_unknown_body::<RecordingReporter>(Some(
        &json!({ "authUrl": "https://example.com/auth", "doneUrl": true }),
    ));
    assert!(warns().iter().any(|message| message.contains("doneUrl")), "got {:?}", warns());
    let body = error.as_otp_challenge().expect("a challenge").body.expect("a body");
    assert_eq!(body.auth_url, Some("https://example.com/auth".to_owned()));
    assert_eq!(body.done_url, None);
}

#[test]
fn from_unknown_body_warns_for_both_when_both_have_wrong_types() {
    reset();
    let error = SyntheticOtpError::from_unknown_body::<RecordingReporter>(Some(
        &json!({ "authUrl": 42, "doneUrl": false }),
    ));
    assert!(warns().iter().any(|message| message.contains("authUrl")), "got {:?}", warns());
    assert!(warns().iter().any(|message| message.contains("doneUrl")), "got {:?}", warns());
    let body = error.as_otp_challenge().expect("a challenge").body.expect("a body");
    assert_eq!(body.auth_url, None);
    assert_eq!(body.done_url, None);
}

#[test]
fn from_unknown_body_returns_empty_body_when_no_auth_url_or_done_url() {
    let error = SyntheticOtpError::from_unknown_body::<UnexpectedReporter>(Some(
        &json!({ "something": "else" }),
    ));
    assert_eq!(
        error.as_otp_challenge().expect("a challenge").body,
        Some(OtpErrorBody { auth_url: None, done_url: None }),
    );
}
