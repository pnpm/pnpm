use crate::{
    WebAuthTimeoutError,
    capabilities::{Clock, Sleep, WebAuthFetch},
};

#[cfg(test)]
mod tests;

/// Options forwarded to each poll request (the `method` is always `GET`, so
/// it is implicit here). `retry` carries the undici retry knobs; the
/// production [`Host`](crate::Host) fetch currently applies only `timeout`
/// and leaves `retry` for the consuming command to wire.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WebAuthFetchOptions {
    /// Per-request timeout in milliseconds.
    pub timeout: Option<u64>,
    pub retry: Option<WebAuthRetryOptions>,
}

/// Retry knobs forwarded with [`WebAuthFetchOptions`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WebAuthRetryOptions {
    pub factor: Option<f64>,
    pub max_timeout: Option<u64>,
    pub min_timeout: Option<u64>,
    pub randomize: Option<bool>,
    pub retries: Option<u32>,
}

/// A poll response materialized by the [`WebAuthFetch`] capability:
/// `ok` / `status`, the one header the poll reads (`Retry-After`), and the
/// body bytes that [`token`](Self::token) decodes and parses.
#[derive(Debug, Clone)]
pub struct WebAuthFetchResponse {
    pub ok: bool,
    pub status: u16,
    /// Value of the `Retry-After` response header, if present.
    pub retry_after: Option<String>,
    /// Raw response body bytes, materialized only for responses whose body
    /// the poll reads (see [`body_may_carry_token`]) and capped at the size
    /// the provider enforces. [`token`](Self::token) decodes and parses them,
    /// so the provider stays a thin transport that copies bytes without
    /// interpreting them.
    pub body: Vec<u8>,
    /// Whether the registry's body exceeded the provider's read cap and was
    /// therefore not fully materialized. A capped body cannot be parsed as
    /// the intended small token object, so [`token`](Self::token) reports it
    /// as carrying no token. Surfacing the cap here — rather than discarding
    /// the body inside the provider — keeps the size-limit feature reachable
    /// through the `WebAuthFetch` dependency-injection seam that the `login`
    /// and `publish` flows are tested with.
    pub truncated: bool,
}

impl WebAuthFetchResponse {
    /// Extract the `token` field from the JSON body. `Ok(None)` when the
    /// body parses but carries no token or when it was
    /// [`truncated`](Self::truncated) at the read cap; `Err` when the body
    /// is not the expected JSON shape, which the poll loop swallows.
    pub fn token(&self) -> Result<Option<String>, serde_json::Error> {
        if self.truncated {
            return Ok(None);
        }
        #[derive(serde::Deserialize)]
        struct TokenBody {
            #[serde(default)]
            token: Option<String>,
        }
        serde_json::from_str::<TokenBody>(&decode_token_body(&self.body)).map(|body| body.token)
    }
}

/// Decode a token response body the way the TypeScript side's
/// `new TextDecoder().decode(...)` does: losslessly (invalid UTF-8 becomes
/// U+FFFD rather than a hard failure) and stripping a single leading BOM,
/// which `serde_json` would otherwise reject. Living here in the pure,
/// directly-tested [`token`](WebAuthFetchResponse::token) path — rather than
/// in the un-fakeable provider `fetch` — keeps the decode reachable through
/// the [`WebAuthFetch`] dependency-injection seam.
fn decode_token_body(bytes: &[u8]) -> String {
    let mut decoded = String::from_utf8_lossy(bytes).into_owned();
    if decoded.starts_with('\u{FEFF}') {
        decoded.remove(0);
    }
    decoded
}

/// Whether a response with this `ok` / `status` carries the token in its
/// body — i.e. a successful non-`202` response. The poll treats a `202` as
/// "still waiting" and any non-success status as a transient failure, so
/// their bodies are never read; a provider uses this to skip materializing
/// (and, for a live transport, transferring) a body the poll would ignore.
#[must_use]
pub fn body_may_carry_token(ok: bool, status: u16) -> bool {
    ok && status != 202
}

/// Parameters for [`poll_for_web_auth_token`].
#[derive(Debug, Clone)]
pub struct WebAuthTokenPollParams {
    pub done_url: String,
    pub fetch_options: WebAuthFetchOptions,
    /// Overall budget in milliseconds. Defaults to 5 minutes when `None`.
    pub timeout_ms: Option<u64>,
}

const DEFAULT_TIMEOUT_MS: u64 = 5 * 60 * 1000;
const POLL_INTERVAL_MS: u64 = 1000;

/// Poll a registry's "done" URL until it returns an authentication token.
///
/// The caller is responsible for displaying the authentication URL (and
/// optional QR code) before calling this. Returns the token string, or
/// [`WebAuthTimeoutError`] when the budget is exceeded.
pub async fn poll_for_web_auth_token<Sys>(
    params: WebAuthTokenPollParams,
) -> Result<String, WebAuthTimeoutError>
where
    Sys: Clock + Sleep + WebAuthFetch,
{
    let WebAuthTokenPollParams { done_url, fetch_options, timeout_ms } = params;
    let timeout_ms = timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    let start_time = Sys::now_ms();

    loop {
        let now = Sys::now_ms();
        if now.saturating_sub(start_time) > timeout_ms {
            return Err(WebAuthTimeoutError::new(now, start_time, timeout_ms));
        }

        Sys::sleep_ms(POLL_INTERVAL_MS).await;

        let Ok(response) = Sys::fetch(&done_url, &fetch_options).await else {
            continue;
        };
        if !response.ok {
            continue;
        }

        if response.status == 202 {
            // Registry is still waiting for authentication.
            wait_for_retry_after::<Sys>(&response, start_time, timeout_ms).await?;
            continue;
        }

        match response.token() {
            Ok(Some(token)) if !token.is_empty() => return Ok(token),
            _ => continue,
        }
    }
}

/// Honor a 202 response's `Retry-After` header by sleeping the
/// *additional* time beyond the poll interval already waited, capped to
/// the remaining budget. Returns `Err` when the budget is already
/// exhausted.
async fn wait_for_retry_after<Sys>(
    response: &WebAuthFetchResponse,
    start_time: u64,
    timeout_ms: u64,
) -> Result<(), WebAuthTimeoutError>
where
    Sys: Clock + Sleep,
{
    let retry_after_seconds = parse_js_number(response.retry_after.as_deref());
    if !retry_after_seconds.is_finite() {
        return Ok(());
    }
    let additional_ms = retry_after_seconds.mul_add(1000.0, -(POLL_INTERVAL_MS as f64));
    if additional_ms <= 0.0 {
        return Ok(());
    }
    let now_after_poll = Sys::now_ms();
    let remaining_ms = timeout_ms as i64
        - i64::try_from(now_after_poll.saturating_sub(start_time)).unwrap_or(i64::MAX);
    if remaining_ms <= 0 {
        return Err(WebAuthTimeoutError::new(now_after_poll, start_time, timeout_ms));
    }
    let sleep_ms = additional_ms.min(remaining_ms as f64);
    Sys::sleep_ms(sleep_ms as u64).await;
    Ok(())
}

/// Mirror JavaScript's `Number(value)` for the `Retry-After` header.
/// Header-absent (`None`) and an empty / whitespace string both map to `0`
/// (as `Number(null)` / `Number('')` do); a non-numeric string maps to
/// `NaN`, so the caller skips the additional wait.
fn parse_js_number(value: Option<&str>) -> f64 {
    match value {
        None => 0.0,
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() { 0.0 } else { trimmed.parse::<f64>().unwrap_or(f64::NAN) }
        }
    }
}
