use std::future::Future;

use pacquet_reporter::Reporter;

use crate::{
    capabilities::{EnterKeyListener, OpenUrl, StdinIsTty},
    global_log::{global_info, global_warn},
};

#[cfg(test)]
mod tests;

/// Race a token-polling future against an optional "press Enter to open in
/// browser" prompt.
///
/// While `poll` runs, an Enter keypress opens `auth_url` in the user's
/// browser. The poll is awaited on its own — the keypress is a
/// fire-and-forget side effect — so authentication that completes on
/// another device (phone QR scan, pasted URL) returns without the user
/// ever pressing Enter. Whatever `poll` resolves to (token or error) is
/// returned, and the listener is always closed.
///
/// Falls back to awaiting `poll` directly — no prompt — when stdin is not a
/// TTY, when `auth_url` is not an `http(s)` URL (it comes from an untrusted
/// registry response), or when the listener fails to set up.
///
/// Ports pnpm's `promptBrowserOpen`.
pub async fn prompt_browser_open<Sys, Reporter, Error, Poll>(
    auth_url: &str,
    poll: Poll,
) -> Result<String, Error>
where
    Sys: StdinIsTty + EnterKeyListener + OpenUrl,
    Reporter: self::Reporter,
    Poll: Future<Output = Result<String, Error>>,
{
    if !Sys::stdin_is_tty() {
        return poll.await;
    }

    let Some(canonical_url) = canonical_http_url(auth_url) else {
        return poll.await;
    };

    let mut listener = match Sys::listen() {
        Ok(listener) => listener,
        Err(error) => {
            global_warn::<Reporter>(&format!("Could not set up keyboard listener: {error}"));
            return poll.await;
        }
    };

    global_info::<Reporter>("Press ENTER to open the URL in your browser.");

    tokio::pin!(poll);
    tokio::select! {
        result = &mut poll => result,
        () = &mut listener => {
            open_in_browser::<Sys, Reporter>(&canonical_url);
            // The keypress fires once; keep awaiting the poll. The listener
            // is dropped (closed) when this function returns.
            poll.await
        }
    }
}

/// Open `url`, downgrading any failure to a warning so a missing
/// `xdg-open` (or equivalent) never interrupts the poll.
fn open_in_browser<Sys, Reporter>(url: &str)
where
    Sys: OpenUrl,
    Reporter: self::Reporter,
{
    if let Err(error) = Sys::open_url(url) {
        global_warn::<Reporter>(&format!("Could not open browser automatically: {error}"));
        global_info::<Reporter>("Please open the URL shown above manually.");
    }
}

/// Return the canonical form of `auth_url` when it is an `http(s)` URL, or
/// `None` otherwise. Guards `open` against `javascript:` / `file:` / other
/// schemes in an untrusted registry response.
fn canonical_http_url(auth_url: &str) -> Option<String> {
    let parsed = url::Url::parse(auth_url).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| parsed.to_string())
}
