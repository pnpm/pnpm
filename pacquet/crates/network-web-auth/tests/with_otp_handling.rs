use std::{cell::Cell, rc::Rc};

use pacquet_network_web_auth::{
    OtpError, OtpErrorBody, SyntheticOtpError, WebAuthFetchOptions, WithOtpError, with_otp_handling,
};
use pacquet_network_web_auth_testing::{
    FakeOtpError, InputResponse, SleepBehavior, ok_202, ok_token, web_auth_body, web_auth_fake,
};
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn returns_the_result_when_the_operation_succeeds_without_otp() {
    web_auth_fake!();
    reset();

    let result = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move { Ok("success".to_owned()) },
    )
    .await
    .expect("a result");

    assert_eq!(result, "success");
}

#[tokio::test]
async fn throws_non_otp_errors_as_is() {
    web_auth_fake!();
    reset();

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move { Err(FakeOtpError::Other("network error".to_owned())) },
    )
    .await
    .expect_err("an error");

    assert!(
        matches!(&error, WithOtpError::Operation(FakeOtpError::Other(message)) if message == "network error"),
        "expected the original error, got {error:?}",
    );
}

#[tokio::test]
async fn throws_non_interactive_error_when_stdin_is_not_interactive() {
    web_auth_fake!();
    reset();
    set_stdin_tty(false);

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move { Err(FakeOtpError::Otp { body: None }) },
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::NonInteractive(_)), "got {error:?}");
}

#[tokio::test]
async fn throws_non_interactive_error_when_stdout_is_not_interactive() {
    web_auth_fake!();
    reset();
    set_stdout_tty(false);

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move { Err(FakeOtpError::Otp { body: None }) },
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::NonInteractive(_)), "got {error:?}");
}

#[tokio::test]
async fn preserves_web_auth_urls_on_non_interactive_error() {
    web_auth_fake!();
    reset();
    set_stdin_tty(false);

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move { Err(FakeOtpError::Otp { body: web_auth_body() }) },
    )
    .await
    .expect_err("an error");

    match error {
        WithOtpError::NonInteractive(error) => {
            assert_eq!(error.auth_url.as_deref(), Some("https://registry.npmjs.org/auth/abc"));
            assert_eq!(error.done_url.as_deref(), Some("https://registry.npmjs.org/auth/abc/done"));
        }
        other => panic!("expected non-interactive error, got {other:?}"),
    }
}

#[tokio::test]
async fn strips_credentials_from_web_auth_urls_on_non_interactive_error() {
    web_auth_fake!();
    reset();
    set_stdin_tty(false);

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move {
            Err(FakeOtpError::Otp {
                body: Some(OtpErrorBody {
                    auth_url: Some("https://user:secret@registry.npmjs.org/auth/abc".to_owned()),
                    done_url: Some(
                        "https://user:secret@registry.npmjs.org/auth/abc/done?authId=xyz"
                            .to_owned(),
                    ),
                }),
            })
        },
    )
    .await
    .expect_err("an error");

    match error {
        WithOtpError::NonInteractive(error) => {
            assert_eq!(error.auth_url.as_deref(), Some("https://registry.npmjs.org/auth/abc"));
            assert_eq!(
                error.done_url.as_deref(),
                Some("https://registry.npmjs.org/auth/abc/done?authId=xyz"),
            );
        }
        other => panic!("expected non-interactive error, got {other:?}"),
    }
}

#[tokio::test]
async fn omits_non_http_web_auth_urls_on_non_interactive_error() {
    web_auth_fake!();
    reset();
    set_stdin_tty(false);

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move {
            Err(FakeOtpError::Otp {
                body: Some(OtpErrorBody {
                    auth_url: Some("javascript:alert(1)".to_owned()),
                    done_url: Some("file:///tmp/token".to_owned()),
                }),
            })
        },
    )
    .await
    .expect_err("an error");

    match error {
        WithOtpError::NonInteractive(error) => {
            assert_eq!(error.auth_url, None);
            assert_eq!(error.done_url, None);
        }
        other => panic!("expected non-interactive error, got {other:?}"),
    }
}

#[tokio::test]
async fn classic_flow_prompts_for_otp_and_retries_operation() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(Some("654321".to_owned())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let result = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        move |otp| {
            let counter = Rc::clone(&counter);
            async move {
                counter.set(counter.get() + 1);
                if counter.get() == 1 {
                    Err(FakeOtpError::Otp { body: None })
                } else {
                    assert_eq!(otp.as_deref(), Some("654321"));
                    Ok("ok".to_owned())
                }
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
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(Some("123456".to_owned())));

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move { Err(FakeOtpError::Otp { body: None }) },
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::SecondChallenge(_)), "got {error:?}");
}

#[tokio::test]
async fn classic_flow_throws_non_otp_errors_from_the_retry_as_is() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(Some("123456".to_owned())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        move |_otp| {
            let counter = Rc::clone(&counter);
            async move {
                counter.set(counter.get() + 1);
                if counter.get() == 1 {
                    Err(FakeOtpError::Otp { body: None })
                } else {
                    Err(FakeOtpError::Other("server error".to_owned()))
                }
            }
        },
    )
    .await
    .expect_err("an error");

    assert!(
        matches!(&error, WithOtpError::Operation(FakeOtpError::Other(message)) if message == "server error"),
        "got {error:?}",
    );
}

#[tokio::test]
async fn classic_flow_re_throws_the_original_otp_error_when_prompt_returns_empty() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(Some(String::new())));

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move { Err(FakeOtpError::Otp { body: None }) },
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::Operation(FakeOtpError::Otp { .. })), "got {error:?}");
}

#[tokio::test]
async fn classic_flow_re_throws_the_original_otp_error_when_prompt_returns_none() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(None));

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move { Err(FakeOtpError::Otp { body: None }) },
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::Operation(FakeOtpError::Otp { .. })), "got {error:?}");
}

#[tokio::test]
async fn classic_flow_re_throws_the_original_otp_error_when_prompt_is_cancelled() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Cancelled);

    let error = with_otp_handling::<FakeHost, UnexpectedReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        |_otp| async move { Err(FakeOtpError::Otp { body: None }) },
    )
    .await
    .expect_err("an error");

    assert!(matches!(error, WithOtpError::Operation(FakeOtpError::Otp { .. })), "got {error:?}");
}

/// pacquet's Enter-key listener is always available, so the flow also emits
/// the "Press ENTER" line. The assertion checks that the auth URL was
/// surfaced and the token round-tripped, not the exact message count.
#[tokio::test]
async fn web_auth_flow_polls_done_url_and_uses_returned_token() {
    web_auth_fake!();
    reset();
    let fetch_calls = Rc::new(Cell::new(0));
    let fetch_counter = Rc::clone(&fetch_calls);
    set_fetch(Box::new(move || {
        fetch_counter.set(fetch_counter.get() + 1);
        Ok(if fetch_counter.get() < 3 { ok_202() } else { ok_token("web-token-123") })
    }));
    let op_calls = Rc::new(Cell::new(0));
    let op_counter = Rc::clone(&op_calls);

    let result = with_otp_handling::<FakeHost, RecordingReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        move |otp| {
            let op_counter = Rc::clone(&op_counter);
            async move {
                op_counter.set(op_counter.get() + 1);
                if op_counter.get() == 1 {
                    Err(FakeOtpError::Otp { body: web_auth_body() })
                } else {
                    assert_eq!(otp.as_deref(), Some("web-token-123"));
                    Ok("published".to_owned())
                }
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
async fn web_auth_flow_falls_back_to_classic_prompt_when_urls_are_not_http() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(Some("manual-code".to_owned())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let result = with_otp_handling::<FakeHost, RecordingReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        move |otp| {
            let counter = Rc::clone(&counter);
            async move {
                counter.set(counter.get() + 1);
                if counter.get() == 1 {
                    Err(FakeOtpError::Otp {
                        body: Some(OtpErrorBody {
                            auth_url: Some("javascript:alert(1)".to_owned()),
                            done_url: Some("file:///tmp/token".to_owned()),
                        }),
                    })
                } else {
                    assert_eq!(otp.as_deref(), Some("manual-code"));
                    Ok("done".to_owned())
                }
            }
        },
    )
    .await
    .expect("a result");

    assert_eq!(result, "done");
}

#[tokio::test]
async fn web_auth_flow_falls_back_to_classic_prompt_when_only_auth_url_is_present() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(Some("manual-code".to_owned())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let result = with_otp_handling::<FakeHost, RecordingReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        move |otp| {
            let counter = Rc::clone(&counter);
            async move {
                counter.set(counter.get() + 1);
                if counter.get() == 1 {
                    Err(FakeOtpError::Otp {
                        body: Some(OtpErrorBody {
                            auth_url: Some("https://registry.npmjs.org/auth/abc".to_owned()),
                            done_url: None,
                        }),
                    })
                } else {
                    assert_eq!(otp.as_deref(), Some("manual-code"));
                    Ok("done".to_owned())
                }
            }
        },
    )
    .await
    .expect("a result");

    assert_eq!(result, "done");
}

#[tokio::test]
async fn web_auth_flow_falls_back_to_classic_prompt_when_only_done_url_is_present() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(Some("manual-code".to_owned())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let result = with_otp_handling::<FakeHost, RecordingReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        move |otp| {
            let counter = Rc::clone(&counter);
            async move {
                counter.set(counter.get() + 1);
                if counter.get() == 1 {
                    Err(FakeOtpError::Otp {
                        body: Some(OtpErrorBody {
                            auth_url: None,
                            done_url: Some("https://registry.npmjs.org/auth/abc/done".to_owned()),
                        }),
                    })
                } else {
                    assert_eq!(otp.as_deref(), Some("manual-code"));
                    Ok("done".to_owned())
                }
            }
        },
    )
    .await
    .expect("a result");

    assert_eq!(result, "done");
}

#[tokio::test]
async fn web_auth_flow_throws_timeout_error_when_polling_times_out() {
    web_auth_fake!();
    reset();
    set_sleep_behavior(SleepBehavior::AdvanceByFixed(6 * 60 * 1000));
    set_fetch(Box::new(|| Ok(ok_202())));
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);

    let error = with_otp_handling::<FakeHost, RecordingReporter, String, FakeOtpError, _, _>(
        WebAuthFetchOptions::default(),
        move |_otp| {
            let counter = Rc::clone(&counter);
            async move {
                counter.set(counter.get() + 1);
                assert_eq!(counter.get(), 1, "the operation must not be retried after a timeout");
                Err(FakeOtpError::Otp { body: web_auth_body() })
            }
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
    web_auth_fake!();
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
    web_auth_fake!();
    let error = SyntheticOtpError::from_unknown_body::<UnexpectedReporter>(Some(&json!(null)));
    assert_eq!(error.as_otp_challenge().expect("a challenge").body, None);
}

#[test]
fn from_unknown_body_returns_no_body_when_body_is_not_an_object() {
    web_auth_fake!();
    let error =
        SyntheticOtpError::from_unknown_body::<UnexpectedReporter>(Some(&json!("not an object")));
    assert_eq!(error.as_otp_challenge().expect("a challenge").body, None);
}

#[test]
fn from_unknown_body_warns_when_auth_url_has_wrong_type() {
    web_auth_fake!();
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
    web_auth_fake!();
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
    web_auth_fake!();
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
    web_auth_fake!();
    let error = SyntheticOtpError::from_unknown_body::<UnexpectedReporter>(Some(
        &json!({ "something": "else" }),
    ));
    assert_eq!(
        error.as_otp_challenge().expect("a challenge").body,
        Some(OtpErrorBody { auth_url: None, done_url: None }),
    );
}
