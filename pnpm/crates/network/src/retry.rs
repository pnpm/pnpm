//! Shared request-retry policy for registry network access.
//!
//! pnpm wraps every registry request — metadata *and* tarball — in
//! `@zkochan/retry` with one retry-timeout budget sourced from the
//! `fetch-retries` family of settings. This module is pacquet's single
//! home for that budget and its exponential-backoff math, so the
//! metadata fetchers, pnpr's upstream proxy, and the tarball downloader
//! all share one [`RetryOpts`] type and one algorithm.
//!
//! [`send_with_retry`] is the one-HTTP-round-trip helper the metadata
//! fetchers and pnpr use. The tarball path keeps its own loop — its
//! retry boundary wraps the whole fetch + integrity + decode + extract
//! pipeline, not just a single request — but reuses [`RetryOpts`].
//!
//! Reading a response body can fail *after* [`send_with_retry`] has
//! handed back a `200`, outside its loop; [`retry_async`] is the
//! companion that re-issues the whole request when consuming or parsing
//! the body fails. See its docs for why that second layer exists.

use std::{future::Future, time::Duration};

use reqwest::{Client, RequestBuilder, Response, StatusCode};

use crate::{ThrottledClient, ThrottledClientGuard, redact_url_credentials};

/// Settings for the per-request retry loop. Maps to the
/// `fetch-retries` / `fetch-retry-factor` / `fetch-retry-mintimeout` /
/// `fetch-retry-maxtimeout` settings and the `@zkochan/retry`
/// algorithm:
///
/// `delay = min(min_timeout * factor.pow(attempt), max_timeout)`
///
/// `attempt` is zero-indexed, so the first post-failure wait is
/// `min_timeout`. `retries` is the number of *retries* — total
/// attempts is `retries + 1`.
///
/// # Pathological configurations
///
/// We don't sanitize these here because pnpm doesn't either — the
/// config plumbing is meant to be byte-equivalent to upstream. The
/// total number of attempts is always bounded by `retries`, so even a
/// degenerate `delay_for` only removes the backoff:
///
/// * `factor == 0` keeps the first wait at `min_timeout` (`0u32.pow(0)
///   == 1`), but every subsequent wait is `0` — i.e. no backoff
///   between retries. Same as pnpm.
/// * `factor == 1` waits `min_timeout` between every attempt. Same as
///   pnpm.
/// * `max_timeout < min_timeout` makes every wait `max_timeout`. Same
///   as pnpm.
///
/// If a caller wants stricter validation (warn / reject these configs),
/// it belongs above the `Config` boundary, alongside any other npmrc
/// sanity checks pnpm grows over time.
///
/// Defaults: 2 retries, factor 10, 10 s floor, 60 s cap.
#[derive(Debug, Clone, Copy)]
pub struct RetryOpts {
    pub retries: u32,
    pub factor: u32,
    pub min_timeout: Duration,
    pub max_timeout: Duration,
}

impl Default for RetryOpts {
    fn default() -> Self {
        Self {
            retries: 2,
            factor: 10,
            min_timeout: Duration::from_secs(10),
            max_timeout: Duration::from_mins(1),
        }
    }
}

impl RetryOpts {
    /// Backoff to wait before the `(attempt + 1)`-th attempt, where
    /// `attempt` is the zero-indexed number of failures so far. Matches
    /// `@zkochan/retry`'s formula with `randomize: false`.
    #[must_use]
    pub fn delay_for(self, attempt: u32) -> Duration {
        // `Duration::as_millis` returns `u128` because a `Duration` can
        // hold values that overflow `u64` milliseconds, but
        // `Duration::from_millis` only takes `u64`. Saturate on the way
        // down so a pathological caller-supplied timeout produces the
        // largest expressible delay rather than a silently truncated
        // one.
        let min_ms = u64::try_from(self.min_timeout.as_millis()).unwrap_or(u64::MAX);
        let max_ms = u64::try_from(self.max_timeout.as_millis()).unwrap_or(u64::MAX);
        let pow = u64::from(self.factor).checked_pow(attempt).unwrap_or(u64::MAX);
        Duration::from_millis(min_ms.saturating_mul(pow).min(max_ms))
    }
}

/// Registry responses worth retrying: request timeout (408), too many
/// requests (429), and any 5xx. Matches the retryable set
/// `make-fetch-happen` applies under pnpm's metadata fetch.
#[must_use]
pub fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

/// Issue the request built by `build_request` against `url`, retrying
/// transport errors and retryable responses ([`should_retry_status`])
/// under `retry_opts`'s exponential backoff.
///
/// Returns the final [`Response`] of *any* status — including 304, 404,
/// and non-retryable 4xx — paired with the [`ThrottledClientGuard`]
/// whose permit was held for the winning attempt. The caller inspects
/// the status (`error_for_status`, 304/404 short-circuits, ...) and keeps
/// the guard alive through body streaming; dropping it earlier would
/// stop the semaphore from bounding the real concurrent socket count
/// (see [`ThrottledClientGuard`]).
///
/// The network permit is acquired once per attempt *inside* the loop,
/// and both the response and its guard are dropped before each backoff
/// sleep, so a flapping registry never pins a socket or parks a
/// concurrency permit during the wait.
pub async fn send_with_retry<'client>(
    http_client: &'client ThrottledClient,
    url: &str,
    retry_opts: RetryOpts,
    mut build_request: impl FnMut(&Client) -> RequestBuilder,
) -> Result<(ThrottledClientGuard<'client>, Response), reqwest::Error> {
    let mut attempt = 0;
    loop {
        let client = http_client.acquire_for_url(url).await;
        match build_request(&client).send().await {
            Ok(response)
                if should_retry_status(response.status()) && attempt < retry_opts.retries =>
            {
                let status = response.status();
                drop(response);
                drop(client);
                let delay = retry_opts.delay_for(attempt);
                tracing::warn!(
                    target: "pacquet_network::retry",
                    url = %redact_url_credentials(url),
                    ?status,
                    attempt = attempt + 1,
                    max_attempts = retry_opts.retries + 1,
                    ?delay,
                    "Request failed; retrying after backoff",
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Ok(response) => return Ok((client, response)),
            Err(error) if attempt < retry_opts.retries => {
                drop(client);
                let delay = retry_opts.delay_for(attempt);
                // reqwest embeds the full request URL in its error, which can
                // carry a secret in the path (e.g. `logout`'s revoke token).
                // The `url=` field already logs the URL the caller handed us
                // (token-free for such callers), so drop the URL from the
                // error to keep it out of the log.
                let error = error.without_url();
                tracing::warn!(
                    target: "pacquet_network::retry",
                    url = %redact_url_credentials(url),
                    error = %redact_url_credentials(&format!("{error:?}")),
                    attempt = attempt + 1,
                    max_attempts = retry_opts.retries + 1,
                    ?delay,
                    "Request errored; retrying after backoff",
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Run `attempt` — a full "issue the request, then read and parse its
/// body" round trip — retrying under `retry_opts`'s exponential
/// backoff whenever it fails with an error `is_retryable` accepts and
/// retries remain.
///
/// This is the body-read/parse companion to [`send_with_retry`].
/// pnpm's metadata fetch nests two retry layers: the shared network
/// library retries the request ([`send_with_retry`] here), and the
/// resolver re-runs the *whole* fetch when reading or parsing the
/// response body fails — "error decoding response body" from a
/// mid-stream reset, or broken JSON — via a second `@zkochan/retry`
/// operation. The `attempt` closure issues the request *and* consumes
/// its body,
/// so `is_retryable` should accept only body-read/parse failures:
/// transport and status failures stay non-retryable here because
/// [`send_with_retry`] inside the closure already owns that budget,
/// exactly as pnpm rejects a fetch failure immediately while letting
/// the network library retry it internally.
pub async fn retry_async<Value, Err, Fut>(
    url: &str,
    retry_opts: RetryOpts,
    is_retryable: impl Fn(&Err) -> bool,
    mut attempt_fn: impl FnMut() -> Fut,
) -> Result<Value, Err>
where
    Fut: Future<Output = Result<Value, Err>>,
    Err: std::fmt::Debug,
{
    let mut attempt = 0;
    loop {
        match attempt_fn().await {
            Ok(value) => return Ok(value),
            Err(error) if is_retryable(&error) && attempt < retry_opts.retries => {
                let delay = retry_opts.delay_for(attempt);
                tracing::warn!(
                    target: "pacquet_network::retry",
                    url = %redact_url_credentials(url),
                    error = %redact_url_credentials(&format!("{error:?}")),
                    attempt = attempt + 1,
                    max_attempts = retry_opts.retries + 1,
                    ?delay,
                    "Reading response body failed; retrying after backoff",
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests;
