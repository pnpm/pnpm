use crate::{
    SharedReportedProgressKeys, TarballError,
    download::{emit_progress_fetched, is_transient_error, tarball_error_to_request_retry},
};
use pnpm_network::{RetryOpts, redact_url_for_display};
use pnpm_reporter::{LogEvent, LogLevel, Reporter, RequestRetryLog};
use std::future::Future;

/// Retry the complete fetch, verification and extraction attempt. Failed
/// attempts cannot publish store rows; successful attempts emit fetched once.
pub(crate) async fn retry_archive<Reporter, Output, Attempt>(
    package_url: &str,
    package_id: &str,
    requester: &str,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    retry_opts: RetryOpts,
    mut fetch: impl FnMut(u32) -> Attempt,
) -> Result<Output, TarballError>
where
    Reporter: self::Reporter,
    Attempt: Future<Output = Result<Output, TarballError>>,
{
    let max_retries = retry_opts.retries;
    let mut attempt: u32 = 0;
    loop {
        let result = fetch(attempt).await;
        match result {
            Ok(value) => {
                emit_progress_fetched::<Reporter>(package_id, requester, progress_key);
                return Ok(value);
            }
            Err(err) if !is_transient_error(&err) => return Err(err),
            Err(err) if attempt >= max_retries => {
                tracing::warn!(
                    target: "pacquet::download",
                    package_url = %redact_url_for_display(package_url),
                    attempts = u64::from(attempt) + 1,
                    %err,
                    "Archive fetch retry budget exhausted",
                );
                return Err(err);
            }
            Err(err) => {
                let delay = retry_opts.delay_for(attempt);
                tracing::warn!(
                    target: "pacquet::download",
                    package_url = %redact_url_for_display(package_url),
                    attempt = u64::from(attempt) + 1,
                    max_attempts = u64::from(max_retries) + 1,
                    ?delay,
                    %err,
                    "Archive fetch failed; retrying after backoff",
                );
                Reporter::emit(&LogEvent::RequestRetry(RequestRetryLog {
                    level: LogLevel::Debug,
                    attempt: attempt.saturating_add(1),
                    error: tarball_error_to_request_retry(&err),
                    max_retries,
                    method: "GET".to_string(),
                    timeout: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    url: redact_url_for_display(package_url),
                }));
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}
