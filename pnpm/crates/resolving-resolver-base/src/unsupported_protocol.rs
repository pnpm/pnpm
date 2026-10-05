use derive_more::{Display, Error};
use miette::Diagnostic;

/// `ERR_PNPM_UNSUPPORTED_PROTOCOL`: a specifier opens with a protocol that no
/// resolver supports, such as Yarn's `patch:`, and no directory exists at the
/// local path it was read as. Boxed outermost so the tree walker can
/// recover the code by downcast.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Unsupported protocol {protocol:?} in the dependency specifier {specifier:?}")]
#[diagnostic(
    code(ERR_PNPM_UNSUPPORTED_PROTOCOL),
    help(
        r#"If a published package declares this dependency, replace its specifier with the "overrides" setting."#
    )
)]
pub struct UnsupportedProtocolError {
    #[error(not(source))]
    pub specifier: String,
    pub protocol: String,
}

impl UnsupportedProtocolError {
    /// The error for `specifier` when it opens with a protocol of two or
    /// more characters, `None` otherwise. A single letter before the colon
    /// is a Windows drive.
    #[must_use]
    pub fn detect(specifier: &str) -> Option<Self> {
        let (scheme, _) = specifier.split_once(':')?;
        let mut chars = scheme.chars();
        let is_scheme = scheme.len() >= 2
            && chars.next().is_some_and(|first| first.is_ascii_alphabetic())
            && chars.all(|char| char.is_ascii_alphanumeric() || matches!(char, '+' | '-' | '.'));
        is_scheme.then(|| Self { specifier: specifier.to_string(), protocol: format!("{scheme}:") })
    }
}

#[cfg(test)]
mod tests;
