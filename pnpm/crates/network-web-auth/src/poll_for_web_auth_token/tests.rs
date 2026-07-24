use std::{
    cell::{Cell, RefCell},
    future::{self, Future},
    rc::Rc,
};

use pipe_trait::Pipe;
use pretty_assertions::assert_eq;

use super::{
    WebAuthFetchOptions, WebAuthFetchResponse, WebAuthRetryOptions, WebAuthTokenPollParams,
    body_may_carry_token, poll_for_web_auth_token,
};
use crate::capabilities::{Clock, Sleep, WebAuthFetch, WebAuthFetchError};

#[test]
fn body_may_carry_token_only_for_a_successful_non_202() {
    assert!(body_may_carry_token(true, 200));
    assert!(!body_may_carry_token(true, 202), "202 is still-waiting, its body is skipped");
    assert!(!body_may_carry_token(false, 404), "a failed response's body is skipped");
    assert!(!body_may_carry_token(false, 200), "a non-ok status wins even at 200");
}

#[test]
fn token_reads_the_body_when_not_truncated() {
    let response = ok_token("tok");
    assert_eq!(response.token().expect("valid JSON body"), Some("tok".to_owned()));
}

#[test]
fn token_ignores_a_truncated_body_even_when_it_carries_a_token() {
    let response = ok_truncated();
    assert!(!response.body.is_empty(), "the fixture's body does carry a token");
    assert_eq!(response.token().expect("truncation short-circuits before parsing"), None);
}

/// An invalid UTF-8 byte decodes losslessly (it becomes U+FFFD) so an
/// otherwise-parsable token still comes through, matching the TypeScript
/// side's non-fatal `TextDecoder`. A strict decode would drop the body.
#[test]
fn token_decodes_an_invalid_utf8_body_lossily() {
    let mut body = br#"{"token":"a"#.to_vec();
    body.push(0xFF);
    body.extend_from_slice(br#"b"}"#);
    let response =
        WebAuthFetchResponse { ok: true, status: 200, retry_after: None, body, truncated: false };
    assert_eq!(
        response.token().expect("parse the lossily-decoded body"),
        Some("a\u{FFFD}b".to_owned()),
    );
}

/// A leading UTF-8 BOM is stripped before parsing, matching the TypeScript
/// side's `TextDecoder`; `serde_json` rejects a BOM, so without stripping an
/// otherwise-valid token body would be dropped.
#[test]
fn token_strips_a_leading_bom() {
    let mut body = vec![0xEF, 0xBB, 0xBF];
    body.extend_from_slice(br#"{"token":"tok"}"#);
    let response =
        WebAuthFetchResponse { ok: true, status: 200, retry_after: None, body, truncated: false };
    assert_eq!(response.token().expect("parse the BOM-prefixed body"), Some("tok".to_owned()));
}

/// A scripted stand-in for one `fetch` call, given the request URL and
/// options so a test can both decide the response and capture the inputs.
type FetchScript =
    Box<dyn FnMut(&str, &WebAuthFetchOptions) -> Result<WebAuthFetchResponse, WebAuthFetchError>>;

/// How the [`Sleep`] fake moves the [`Clock`] fake forward. The two share
/// the `TIME` cell so a test can drive the timeout deterministically —
/// mirroring the TS tests where `setTimeout` mutates the same `time`
/// variable `Date.now` reads.
#[derive(Clone, Copy)]
enum SleepBehavior {
    /// `Date.now` is pinned; sleeping records the delay but does not move
    /// the clock.
    NoAdvance,
    /// Sleeping advances the clock by the requested milliseconds.
    AdvanceByMs,
    /// Sleeping advances the clock by a fixed amount regardless of the
    /// requested delay (the TS `setTimeout: () => { time += K }` shape).
    AdvanceByFixed(u64),
}

// Per-test fake for the web-auth clock, sleeps, and fetch. Its state lives in
// fn-local thread-locals, so each `#[test]` gets independent storage and
// concurrent tests never share the clock or the recorded sleeps. Each test
// names the optional helpers it drives, so every emitted helper is used and
// none needs a `dead_code` allow.
macro_rules! poll_fake {
    ($($helper:ident),* $(,)?) => {
        thread_local! {
            static TIME: Cell<u64> = const { Cell::new(0) };
            static SLEEP_BEHAVIOR: Cell<SleepBehavior> =
                const { Cell::new(SleepBehavior::NoAdvance) };
            static SLEEPS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
            static FETCH: RefCell<Option<FetchScript>> = const { RefCell::new(None) };
        }

        struct Fake;

        impl Clock for Fake {
            fn now_ms() -> u64 {
                TIME.with(Cell::get)
            }
        }

        impl Sleep for Fake {
            fn sleep_ms(ms: u64) -> impl Future<Output = ()> {
                SLEEPS.with(|sleeps| sleeps.borrow_mut().push(ms));
                let delta = match SLEEP_BEHAVIOR.with(Cell::get) {
                    SleepBehavior::NoAdvance => 0,
                    SleepBehavior::AdvanceByMs => ms,
                    SleepBehavior::AdvanceByFixed(jump) => jump,
                };
                if delta != 0 {
                    TIME.with(|time| time.set(time.get().saturating_add(delta)));
                }
                future::ready(())
            }
        }

        impl WebAuthFetch for Fake {
            fn fetch(
                url: &str,
                options: &WebAuthFetchOptions,
            ) -> impl Future<Output = Result<WebAuthFetchResponse, WebAuthFetchError>> {
                let result = FETCH.with(|fetch| {
                    let mut fetch = fetch.borrow_mut();
                    let script = fetch.as_mut().expect("a fetch script must be installed");
                    script(url, options)
                });
                future::ready(result)
            }
        }

        // Reset the fake state, in case the same test is re-run within the
        // same process on retry.
        fn reset() {
            TIME.with(|time| time.set(0));
            SLEEP_BEHAVIOR.with(|behavior| behavior.set(SleepBehavior::NoAdvance));
            SLEEPS.with(|sleeps| sleeps.borrow_mut().clear());
            FETCH.with(|fetch| *fetch.borrow_mut() = None);
        }

        fn set_fetch(script: FetchScript) {
            FETCH.with(|fetch| *fetch.borrow_mut() = Some(script));
        }

        $( poll_fake!(@helper $helper); )*
    };

    (@helper set_sleep_behavior) => {
        fn set_sleep_behavior(behavior: SleepBehavior) {
            SLEEP_BEHAVIOR.with(|cell| cell.set(behavior));
        }
    };
    (@helper recorded_sleeps) => {
        fn recorded_sleeps() -> Vec<u64> {
            SLEEPS.with(|sleeps| sleeps.borrow().clone())
        }
    };
    (@helper $unknown:ident) => {
        compile_error!(concat!(
            "unknown `poll_fake!` helper `",
            stringify!($unknown),
            "`; expected one of: set_sleep_behavior, recorded_sleeps",
        ));
    };
}

fn ok_202(retry_after: Option<&str>) -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: true,
        status: 202,
        retry_after: retry_after.map(str::to_owned),
        body: b"{}".to_vec(),
        truncated: false,
    }
}

fn ok_token(token: &str) -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: true,
        status: 200,
        retry_after: None,
        body: serde_json::json!({ "token": token }).to_string().into_bytes(),
        truncated: false,
    }
}

fn ok_json(body: &serde_json::Value) -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: true,
        status: 200,
        retry_after: None,
        body: body.to_string().into_bytes(),
        truncated: false,
    }
}

/// A `200` response whose body the provider capped at the size limit. Its
/// body carries a real token, so a poll that still ignores it proves the
/// truncation — not a missing token — is what makes it keep waiting.
fn ok_truncated() -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: true,
        status: 200,
        retry_after: None,
        body: serde_json::json!({ "token": "tok" }).to_string().into_bytes(),
        truncated: true,
    }
}

fn not_ok() -> WebAuthFetchResponse {
    WebAuthFetchResponse {
        ok: false,
        status: 404,
        retry_after: None,
        body: Vec::new(),
        truncated: false,
    }
}

fn params(timeout_ms: Option<u64>) -> WebAuthTokenPollParams {
    WebAuthTokenPollParams {
        done_url: "https://registry.npmjs.org/auth/done".to_owned(),
        fetch_options: WebAuthFetchOptions::default(),
        timeout_ms,
    }
}

#[tokio::test]
async fn returns_token_when_done_url_responds_with_200_and_token() {
    poll_fake!();
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() < 3 { ok_202(Some("1")) } else { ok_token("web-token-123") })
    }));

    let token = poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(token, "web-token-123");
    assert_eq!(calls.get(), 3);
}

#[tokio::test]
async fn passes_done_url_and_fetch_options_to_fetch() {
    poll_fake!();
    reset();
    let captured = Rc::new(RefCell::new(Vec::<(String, WebAuthFetchOptions)>::new()));
    let sink = Rc::clone(&captured);
    set_fetch(Box::new(move |url, options| {
        sink.borrow_mut().push((url.to_owned(), options.clone()));
        Ok(ok_token("tok"))
    }));
    let options = WebAuthFetchOptions {
        timeout: Some(5000),
        retry: Some(WebAuthRetryOptions { retries: Some(3), ..WebAuthRetryOptions::default() }),
    };

    poll_for_web_auth_token::<Fake>(WebAuthTokenPollParams {
        done_url: "https://registry.example.com/done".to_owned(),
        fetch_options: options.clone(),
        timeout_ms: None,
    })
    .await
    .expect("a token");

    assert_eq!(*captured.borrow(), vec![("https://registry.example.com/done".to_owned(), options)]);
}

#[tokio::test]
async fn respects_retry_after_header_when_polling() {
    poll_fake!(recorded_sleeps);
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() == 1 { ok_202(Some("5")) } else { ok_token("tok") })
    }));

    poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    // First the 1s poll interval, then the 5s Retry-After minus the 1s
    // already waited, then the next iteration's 1s poll interval.
    assert_eq!(recorded_sleeps(), vec![1000, 4000, 1000]);
}

#[tokio::test]
async fn ignores_retry_after_when_value_is_not_a_finite_number() {
    poll_fake!(recorded_sleeps);
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() == 1 { ok_202(Some("not-a-number")) } else { ok_token("tok") })
    }));

    poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(recorded_sleeps(), vec![1000, 1000]);
}

#[tokio::test]
async fn ignores_retry_after_when_value_is_absent() {
    poll_fake!(recorded_sleeps);
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() == 1 { ok_202(None) } else { ok_token("tok") })
    }));

    poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(recorded_sleeps(), vec![1000, 1000]);
}

#[tokio::test]
async fn skips_additional_delay_when_retry_after_is_less_than_poll_interval() {
    poll_fake!(recorded_sleeps);
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() == 1 { ok_202(Some("0.5")) } else { ok_token("tok") })
    }));

    poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(recorded_sleeps(), vec![1000, 1000]);
}

#[tokio::test]
async fn caps_retry_after_additional_delay_to_remaining_timeout() {
    poll_fake!(set_sleep_behavior, recorded_sleeps);
    reset();
    set_sleep_behavior(SleepBehavior::AdvanceByMs);
    set_fetch(Box::new(|_url, _options| Ok(ok_202(Some("60")))));

    // A 10s budget so the 60s Retry-After gets capped.
    poll_for_web_auth_token::<Fake>(params(Some(10_000)))
        .await
        .expect_err("polling should time out");

    let sleeps = recorded_sleeps();
    assert_eq!(sleeps[0], 1000);
    assert!(sleeps[1] <= 9000, "additional delay capped to the remaining budget, got {sleeps:?}");
}

#[tokio::test]
async fn throws_timeout_error_when_timeout_expires_during_retry_after_wait() {
    poll_fake!(set_sleep_behavior);
    reset();
    set_sleep_behavior(SleepBehavior::AdvanceByMs);
    set_fetch(Box::new(|_url, _options| Ok(ok_202(Some("100")))));

    let error = poll_for_web_auth_token::<Fake>(params(Some(5000)))
        .await
        .expect_err("polling should time out");

    assert_eq!(error.timeout, 5000);
}

#[tokio::test]
async fn continues_polling_when_fetch_fails() {
    poll_fake!();
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        if counter.get() == 1 { Err(WebAuthFetchError) } else { Ok(ok_token("tok")) }
    }));

    let token = poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(token, "tok");
    assert_eq!(calls.get(), 2);
}

#[tokio::test]
async fn continues_polling_when_response_is_not_ok() {
    poll_fake!();
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() == 1 { not_ok() } else { ok_token("tok") })
    }));

    let token = poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(token, "tok");
    assert_eq!(calls.get(), 2);
}

#[tokio::test]
async fn continues_polling_when_response_body_is_not_json() {
    poll_fake!();
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() == 1 {
            WebAuthFetchResponse {
                ok: true,
                status: 200,
                retry_after: None,
                body: b"not json".to_vec(),
                truncated: false,
            }
        } else {
            ok_token("tok")
        })
    }));

    let token = poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(token, "tok");
    assert_eq!(calls.get(), 2);
}

/// A `200` whose body was capped at the read limit (`truncated`) is ignored
/// even though its body carries a token, so the poll keeps waiting and only
/// returns once an untruncated token arrives. Driving this through the
/// [`WebAuthFetch`] fake is what makes the size-limit feature testable
/// without the real HTTP transport.
#[tokio::test]
async fn continues_polling_when_response_body_was_truncated() {
    poll_fake!();
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() == 1 { ok_truncated() } else { ok_token("tok") })
    }));

    let token = None.pipe(params).pipe(poll_for_web_auth_token::<Fake>).await.expect("a token");

    assert_eq!(token, "tok");
    assert_eq!(calls.get(), 2);
}

#[tokio::test]
async fn continues_polling_when_response_body_has_no_token() {
    poll_fake!();
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() == 1 {
            ok_json(&serde_json::json!({ "something": "else" }))
        } else {
            ok_token("tok")
        })
    }));

    let token = poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(token, "tok");
    assert_eq!(calls.get(), 2);
}

#[tokio::test]
async fn continues_polling_when_token_is_empty_string() {
    poll_fake!();
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() == 1 { ok_token("") } else { ok_token("real-tok") })
    }));

    let token = poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(token, "real-tok");
    assert_eq!(calls.get(), 2);
}

#[tokio::test]
async fn throws_timeout_error_after_timeout() {
    poll_fake!(set_sleep_behavior);
    reset();
    // Jump past the default 5-minute budget on the first sleep.
    set_sleep_behavior(SleepBehavior::AdvanceByFixed(6 * 60 * 1000));
    set_fetch(Box::new(|_url, _options| Ok(ok_202(None))));

    poll_for_web_auth_token::<Fake>(params(None)).await.expect_err("polling should time out");
}

#[tokio::test]
async fn uses_custom_timeout_value() {
    poll_fake!(set_sleep_behavior);
    reset();
    set_sleep_behavior(SleepBehavior::AdvanceByFixed(2000));
    set_fetch(Box::new(|_url, _options| Ok(ok_202(None))));

    let error = poll_for_web_auth_token::<Fake>(params(Some(3000)))
        .await
        .expect_err("polling should time out");

    assert_eq!(error.timeout, 3000);
}

#[tokio::test]
async fn recovers_after_multiple_consecutive_fetch_errors() {
    poll_fake!();
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        if counter.get() <= 5 { Err(WebAuthFetchError) } else { Ok(ok_token("recovered")) }
    }));

    let token = poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(token, "recovered");
    assert_eq!(calls.get(), 6);
}

#[tokio::test]
async fn waits_poll_interval_before_each_fetch_call() {
    poll_fake!(recorded_sleeps);
    reset();
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        Ok(if counter.get() < 4 { ok_202(None) } else { ok_token("tok") })
    }));

    poll_for_web_auth_token::<Fake>(params(None)).await.expect("a token");

    assert_eq!(recorded_sleeps(), vec![1000, 1000, 1000, 1000]);
}

#[tokio::test]
async fn throws_timeout_error_when_remaining_time_is_zero_during_retry_after() {
    poll_fake!(set_sleep_behavior);
    reset();
    set_sleep_behavior(SleepBehavior::AdvanceByMs);
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    set_fetch(Box::new(move |_url, _options| {
        counter.set(counter.get() + 1);
        // The first 202 carries a Retry-After that, after capping to the
        // remaining budget, lands the clock exactly on the timeout.
        Ok(if counter.get() == 1 { ok_202(Some("10")) } else { ok_202(None) })
    }));

    let error = poll_for_web_auth_token::<Fake>(params(Some(2000)))
        .await
        .expect_err("polling should time out");

    assert_eq!(error.timeout, 2000);
}
