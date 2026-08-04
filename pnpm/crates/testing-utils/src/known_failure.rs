/// Marker error for test ports whose subject under test is not implemented yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownFailure {
    pub reason: &'static str,
}

impl KnownFailure {
    #[must_use]
    pub const fn new(reason: &'static str) -> Self {
        Self { reason }
    }
}

pub type KnownResult<Value> = Result<Value, KnownFailure>;

/// Continue a ported test only after the stubbed subject under test is implemented.
#[macro_export]
macro_rules! allow_known_failure {
    ($expr:expr) => {{
        let known_result: $crate::known_failure::KnownResult<_> = $expr;
        match known_result {
            Ok(value) => value,
            Err(_known_failure) => return,
        }
    }};
}
