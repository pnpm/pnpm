use std::{cell::Cell, time::Duration};

use reqwest::StatusCode;

use super::{RetryOpts, retry_async, should_retry_status};

/// `RetryOpts` whose backoff is effectively instant, so retry-loop
/// tests don't sleep.
fn instant_retry_opts(retries: u32) -> RetryOpts {
    RetryOpts {
        retries,
        factor: 1,
        min_timeout: Duration::from_millis(1),
        max_timeout: Duration::from_millis(1),
    }
}

#[test]
fn default_matches_pnpm_fetch_retries() {
    let opts = RetryOpts::default();
    assert_eq!(opts.retries, 2);
    assert_eq!(opts.factor, 10);
    assert_eq!(opts.min_timeout, Duration::from_secs(10));
    assert_eq!(opts.max_timeout, Duration::from_mins(1));
}

#[test]
fn delay_for_grows_exponentially_then_caps_at_max() {
    let opts = RetryOpts {
        retries: 5,
        factor: 10,
        min_timeout: Duration::from_secs(1),
        max_timeout: Duration::from_mins(1),
    };
    assert_eq!(opts.delay_for(0), Duration::from_secs(1), "first wait is min_timeout");
    assert_eq!(opts.delay_for(1), Duration::from_secs(10), "min * factor^1");
    // min * factor^2 = 100_000 ms, capped to max_timeout.
    assert_eq!(opts.delay_for(2), Duration::from_mins(1), "capped at max_timeout");
}

#[test]
fn delay_for_saturates_instead_of_overflowing() {
    let opts = RetryOpts {
        retries: 100,
        factor: 10,
        min_timeout: Duration::from_millis(1),
        max_timeout: Duration::from_millis(u64::MAX),
    };
    // factor.pow(50) overflows u64; saturate to the largest expressible
    // delay rather than wrapping or panicking.
    assert_eq!(opts.delay_for(50), Duration::from_millis(u64::MAX));
}

#[test]
fn retryable_statuses_match_pnpm() {
    assert!(should_retry_status(StatusCode::REQUEST_TIMEOUT)); // 408
    assert!(should_retry_status(StatusCode::TOO_MANY_REQUESTS)); // 429
    assert!(should_retry_status(StatusCode::INTERNAL_SERVER_ERROR)); // 500
    assert!(should_retry_status(StatusCode::BAD_GATEWAY)); // 502
    assert!(should_retry_status(StatusCode::SERVICE_UNAVAILABLE)); // 503

    assert!(!should_retry_status(StatusCode::OK)); // 200
    assert!(!should_retry_status(StatusCode::NOT_MODIFIED)); // 304
    assert!(!should_retry_status(StatusCode::UNAUTHORIZED)); // 401
    assert!(!should_retry_status(StatusCode::FORBIDDEN)); // 403
    assert!(!should_retry_status(StatusCode::NOT_FOUND)); // 404
}

#[tokio::test]
async fn retry_async_retries_a_retryable_error_until_success() {
    let calls = Cell::new(0u32);
    let result: Result<&str, &str> = retry_async(
        "https://registry/pkg",
        instant_retry_opts(3),
        |_error| true,
        || {
            let attempt = calls.get();
            calls.set(attempt + 1);
            async move { if attempt < 2 { Err("error decoding response body") } else { Ok("ok") } }
        },
    )
    .await;
    assert_eq!(result, Ok("ok"));
    assert_eq!(calls.get(), 3, "two failures then a success");
}

#[tokio::test]
async fn retry_async_does_not_retry_a_non_retryable_error() {
    let calls = Cell::new(0u32);
    let result: Result<(), &str> = retry_async(
        "https://registry/pkg",
        instant_retry_opts(3),
        |_error| false,
        || {
            calls.set(calls.get() + 1);
            async { Err("fatal") }
        },
    )
    .await;
    assert_eq!(result, Err("fatal"));
    assert_eq!(calls.get(), 1, "non-retryable errors return on the first attempt");
}

#[tokio::test]
async fn retry_async_gives_up_after_the_retry_budget() {
    let calls = Cell::new(0u32);
    let result: Result<(), &str> = retry_async(
        "https://registry/pkg",
        instant_retry_opts(2),
        |_error| true,
        || {
            calls.set(calls.get() + 1);
            async { Err("error decoding response body") }
        },
    )
    .await;
    assert_eq!(result, Err("error decoding response body"));
    assert_eq!(calls.get(), 3, "initial attempt plus `retries` retries");
}
