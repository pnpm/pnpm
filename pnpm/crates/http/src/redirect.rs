use crate::{Error, Url, error::Kind};
use std::{fmt, sync::Arc};

#[derive(Clone)]
pub struct Policy(Arc<dyn Fn(Attempt<'_>) -> Action + Send + Sync>);
pub struct Attempt<'a> {
    pub(crate) url: &'a Url,
    pub(crate) previous: &'a [Url],
}
pub enum Action {
    Follow,
    Stop,
    Error(Error),
}
impl Policy {
    #[must_use]
    pub fn none() -> Self {
        Self::custom(|_| Action::Stop)
    }
    #[must_use]
    pub fn limited(maximum: usize) -> Self {
        Self::custom(move |attempt| {
            if attempt.previous.len() >= maximum {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        })
    }
    pub fn custom<F>(callback: F) -> Self
    where
        F: Fn(Attempt<'_>) -> Action + Send + Sync + 'static,
    {
        Self(Arc::new(callback))
    }
    pub(crate) fn apply(&self, url: &Url, previous: &[Url]) -> Action {
        (self.0)(Attempt { url, previous })
    }
}
impl Default for Policy {
    fn default() -> Self {
        Self::limited(10)
    }
}
impl fmt::Debug for Policy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Policy")
    }
}
impl Attempt<'_> {
    #[must_use]
    pub fn url(&self) -> &Url {
        self.url
    }
    #[must_use]
    pub fn previous(&self) -> &[Url] {
        self.previous
    }
    #[must_use]
    pub fn follow(self) -> Action {
        Action::Follow
    }
    #[must_use]
    pub fn stop(self) -> Action {
        Action::Stop
    }
    pub fn error(self, error: impl fmt::Display) -> Action {
        Action::Error(Error::new(Kind::Redirect, error.to_string()))
    }
}
