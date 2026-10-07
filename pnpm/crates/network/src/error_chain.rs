use crate::address_guard::BlockedAddress;

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

/// Whether retrying `error` cannot change its outcome, so the retry loops
/// fail the request at once: the server's TLS certificate failed
/// verification (<https://github.com/pnpm/pnpm/issues/9134>), or a
/// [`GuardedDnsResolver`](crate::GuardedDnsResolver) refused the address.
#[must_use]
pub fn is_permanent_error(error: &reqwest::Error) -> bool {
    error_sources(error)
        .any(|error| {
            matches!(
                error.downcast_ref::<rustls::Error>(),
                Some(rustls::Error::InvalidCertificate(_)),
            ) || error.is::<BlockedAddress>()
        })
}

/// Whether `error` is the peer closing the TLS connection without a
/// `close_notify` alert. rustls reports it as an [`std::io::Error`] of kind
/// [`std::io::ErrorKind::UnexpectedEof`] carrying only this message.
pub(crate) fn is_unclean_tls_close(error: &reqwest::Error) -> bool {
    const RUSTLS_UNEXPECTED_EOF_MESSAGE: &str =
        "peer closed connection without sending TLS close_notify";
    error_sources(error)
        .any(|error| {
            error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| {
                    error.kind() == std::io::ErrorKind::UnexpectedEof
                        && error
                            .get_ref()
                            .is_some_and(|inner| {
                                inner.to_string().starts_with(RUSTLS_UNEXPECTED_EOF_MESSAGE)
                            })
                })
        })
}

/// Every error under `error`, including the boxed error of each
/// [`std::io::Error`]: `io::Error::source()` skips it, and that is where
/// the TLS stream keeps the rustls error.
fn error_sources<'a>(
    error: &'a reqwest::Error,
) -> impl Iterator<Item = &'a (dyn std::error::Error + 'static)> {
    std::iter::successors(std::error::Error::source(error), |error| {
        match error.downcast_ref::<std::io::Error>().and_then(std::io::Error::get_ref) {
            Some(inner) => Some(inner as &(dyn std::error::Error + 'static)),
            None => error.source(),
        }
    })
}
