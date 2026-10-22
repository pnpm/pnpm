use super::{CachedTgz, CanonicalPackageName, Storage};
use axum::body::Body;
use futures_util::StreamExt;
use ssri::{Integrity, IntegrityChecker};
use std::io::{self, Read};

/// What to do about a `.tgz` whose bytes did not match the version's
/// integrity: fail the response, which ends after the bytes, and evict the
/// cached copy.
pub(super) struct StaleTgz {
    pub(super) storage: Storage,
    pub(super) name: Option<CanonicalPackageName>,
    pub(super) cached: Option<CachedTgz>,
    pub(super) runtime: tokio::runtime::Handle,
}

impl StaleTgz {
    pub(super) fn evict(self) -> io::Error {
        if let (Some(cached), Some(name)) = (self.cached, self.name) {
            self.runtime.spawn(async move {
                let removed =
                    self.storage.remove_upstream_blob(&cached.namespace, &name, &cached.filename)
                        .await;
                if let Err(err) = removed {
                    tracing::warn!(?err, package = %name.as_str(), "evicting a stale tgz failed");
                }
            });
        }
        io::Error::other("the tarball does not match the integrity its metadata declares")
    }
}

/// `body`, checked against `expected` as it streams. A body that does not
/// match ends with an error and has `stale` evicted.
pub(super) fn verified_body(body: Body, expected: Option<Integrity>, stale: StaleTgz) -> Body {
    let Some(expected) = expected else { return body };
    let state = (body.into_data_stream(), Some((IntegrityChecker::new(expected), stale)));
    let chunks = futures_util::stream::unfold(state, |(mut chunks, mut check)| async move {
        match chunks.next().await {
            Some(Ok(chunk)) => {
                if let Some((checker, _)) = &mut check {
                    checker.input(&chunk);
                }
                Some((Ok(chunk), (chunks, check)))
            }
            Some(Err(err)) => Some((Err(io::Error::other(err)), (chunks, None))),
            None => {
                let (checker, stale) = check?;
                let mismatch = checker
                    .result()
                    .is_err()
                    .then(|| stale.evict());
                mismatch.map(|err| (Err(err), (chunks, None)))
            }
        }
    });
    Body::from_stream(chunks)
}

/// Feeds every byte read through it into an [`IntegrityChecker`], when there
/// is one to match.
pub(super) struct CheckedReader<Inner> {
    pub(super) inner: Inner,
    pub(super) checker: Option<IntegrityChecker>,
}

impl<Inner: Read> Read for CheckedReader<Inner> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = self.inner.read(buf)?;
        if let Some(checker) = &mut self.checker {
            checker.input(&buf[..len]);
        }
        Ok(len)
    }
}
