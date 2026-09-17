//! What looking through this machine's interpreters found for one
//! project, and which of them an install falls back on.

use super::{Interpreter, VersionRequest};
use std::sync::Arc;

/// What searching this machine's interpreters found for one project.
pub(super) enum Search {
    /// One the project's range and its requested version both accept.
    Accepted(Arc<Interpreter>),
    /// One the range accepts, for a version this machine does not have.
    OtherVersion(Arc<Interpreter>),
    /// One the project's range rejects, which only a `runtimeOnFail` that
    /// bypasses the check installs with.
    Unaccepted(Arc<Interpreter>),
    None,
}

/// The interpreters a search keeps to fall back on, each the first of its
/// kind that was found.
#[derive(Default)]
pub(super) struct Fallbacks {
    other_version: Option<Arc<Interpreter>>,
    /// Rejected by the range, but of the version asked for by name. The
    /// conventional names are looked at before the name that version is
    /// installed under, so it is kept apart from the rest to stay the
    /// preferred one.
    requested: Option<Arc<Interpreter>>,
    unaccepted: Option<Arc<Interpreter>>,
}

impl Fallbacks {
    pub(super) fn keep(&mut self, found: Search, request: Option<&VersionRequest>) {
        match found {
            Search::OtherVersion(interpreter) => {
                self.other_version.get_or_insert(interpreter);
            }
            Search::Unaccepted(interpreter) => {
                let asked_for = request.is_none_or(|request| {
                    request.accepts(interpreter.target.environment.python_full_version())
                });
                let kept = if asked_for { &mut self.requested } else { &mut self.unaccepted };
                kept.get_or_insert(interpreter);
            }
            Search::Accepted(_) | Search::None => {}
        }
    }

    /// An interpreter the range accepts is the better fallback: it only
    /// misses the version a `.python-version` file asks for.
    pub(super) fn best(self) -> Search {
        match (self.other_version, self.requested.or(self.unaccepted)) {
            (Some(interpreter), _) => Search::OtherVersion(interpreter),
            (None, Some(interpreter)) => Search::Unaccepted(interpreter),
            (None, None) => Search::None,
        }
    }
}
