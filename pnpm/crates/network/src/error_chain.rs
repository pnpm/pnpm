/// Reqwest's own [`std::fmt::Display`] stops at the outermost stage:
/// `error sending request for url (URL)` or `error decoding response
/// body`, dropping the reason underneath — including `operation timed
/// out` when `fetchTimeout` fired, which leaves the user unable to tell a
/// stalled registry from any other failure.
///
/// [`walk_reqwest_chain`] walks `error.source()` itself and joins every
/// stage's `Display` with `: ` so the rendered message always carries the
/// leaf reason (e.g. `Connection refused (os error 61)`, `operation timed
/// out`, `dns error: failed to lookup address`), regardless of which
/// intermediate `reqwest` / `hyper` / `io::Error` happens to elide it.
#[must_use]
pub fn walk_reqwest_chain(error: &reqwest::Error) -> String {
    let mut out = error.to_string();
    let mut error: &dyn std::error::Error = error;
    while let Some(src) = error.source() {
        let frame = src.to_string();
        // Skip empty or duplicate frames — hyper occasionally repeats
        // the same message across two layers, and reqwest sometimes
        // already includes the inner string in its top-level Display.
        if !frame.is_empty() && !out.ends_with(&frame) {
            out.push_str(": ");
            out.push_str(&frame);
        }
        error = src;
    }
    out
}

/// Whether the server's TLS certificate failed verification. Retrying
/// cannot change the verdict, so the retry loops fail such a request at
/// once (<https://github.com/pnpm/pnpm/issues/9134>).
#[must_use]
pub fn is_certificate_error(error: &reqwest::Error) -> bool {
    let mut source = std::error::Error::source(error);
    while let Some(error) = source {
        if let Some(rustls::Error::InvalidCertificate(_)) = error.downcast_ref::<rustls::Error>() {
            return true;
        }
        // `io::Error::source()` skips its boxed error itself, which is
        // where the TLS stream keeps the rustls error.
        source = match error.downcast_ref::<std::io::Error>().and_then(std::io::Error::get_ref) {
            Some(inner) => Some(inner),
            None => error.source(),
        };
    }
    false
}
