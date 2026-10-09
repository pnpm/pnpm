//! The task cache's tiers as one: the local [`TaskCache`] and, when
//! `pipelineRemoteCache` configures one, the [`RemoteTaskCache`] behind it.

use super::{
    cache::{StoredTask, TaskCache},
    remote_cache::RemoteTaskCache,
};
use pnpm_config::Config;
use std::time::Duration;

#[derive(Clone, Copy)]
pub(super) struct CacheTiers<'a> {
    pub(super) local: &'a TaskCache,
    pub(super) remote: Option<&'a RemoteTaskCache>,
}

impl CacheTiers<'_> {
    /// The entry stored under `key`. On a local miss, the remote tier's entry
    /// is fetched into the local tier first. `warn` hears why the remote tier
    /// could not supply one.
    pub(super) fn lookup(&self, key: &str, warn: impl Fn(&str)) -> Option<StoredTask> {
        if let Some(stored) = self.local.lookup(key) {
            return Some(stored);
        }
        match self.remote?.fetch(key, self.local) {
            Ok(true) => self.local.lookup(key),
            Ok(false) => None,
            Err(reason) => {
                warn(&format!("remote cache: {reason}"));
                None
            }
        }
    }

    /// Start uploading the local entry stored under `key`.
    pub(super) fn upload(&self, key: &str, duration: Duration, warn: impl Fn(&str)) {
        let Some(remote) = self.remote else {
            return;
        };
        let Some(stored) = self.local.lookup(key) else {
            return;
        };
        if let Err(reason) = remote.upload(key, &stored, duration) {
            warn(&format!("remote cache: {reason}"));
        }
    }
}

/// The remote tier, unless `--no-cache` is set or `pipelineRemoteCache`
/// configures none. One that cannot be opened is reported through `warn`, and
/// the run goes on with the local tier alone.
pub(super) fn open_remote_tier(
    config: &Config,
    no_cache: bool,
    warn: impl Fn(String),
) -> Option<RemoteTaskCache> {
    if no_cache {
        return None;
    }
    RemoteTaskCache::open(config)
        .inspect_err(|reason| warn(format!("The remote task cache is off: {reason}")))
        .ok()
        .flatten()
}
