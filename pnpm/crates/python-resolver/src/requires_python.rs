use pep440_rs::VersionSpecifiers;

/// The interpreter range a published file declares, when it declares one
/// that parses.
///
/// A `Requires-Python` pnpm cannot read is treated as absent, so the file
/// stays a candidate. The value belongs to whoever released the
/// distribution, and releases are immutable, so refusing it would put a
/// dependency out of reach of every project that needs it with nothing the
/// project can do about it. pip ignores an unreadable one the same way.
pub(crate) fn declared_range(requires_python: &str) -> Option<VersionSpecifiers> {
    match requires_python.parse() {
        Ok(specifiers) => Some(specifiers),
        Err(error) => {
            tracing::debug!("ignoring invalid Requires-Python {requires_python:?}: {error}");
            None
        }
    }
}
