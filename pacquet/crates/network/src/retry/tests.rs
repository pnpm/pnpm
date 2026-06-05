use std::time::Duration;

use reqwest::StatusCode;

use super::{RetryOpts, should_retry_status};

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
