//! The task cache's tiers as one: the local [`TaskCache`] and, when
//! `remoteCache` configures one, the [`RemoteTaskCache`] behind it.

use super::{
    cache::{StoredTask, TaskCache},
    remote_cache::{RemoteTaskCache, TaskIdentity},
};
use pnpm_config::Config;
use pnpm_workspace_task_scheduler::TaskNode;

#[derive(Clone, Copy)]
pub(super) struct CacheTiers<'a> {
    pub(super) local: &'a TaskCache,
    pub(super) remote: Option<&'a RemoteTaskCache>,
}

impl CacheTiers<'_> {
    /// The entry stored under `key`. On a local miss, the remote tier's entry
    /// is fetched into the local tier first. `warn` hears why the remote tier
    /// could not supply one.
    pub(super) fn lookup(
        &self,
        key: &str,
        node: &TaskNode,
        warn: impl Fn(&str),
    ) -> Option<StoredTask> {
        if let Some(stored) = self.local.lookup(key) {
            return Some(stored);
        }
        let project = self.local.project_rel(&node.project);
        let task = TaskIdentity { project: &project, task: &node.task_name };
        match self.remote?.fetch(key, &task, self.local) {
            Ok(true) => self.local.lookup(key),
            Ok(false) => None,
            Err(reason) => {
                warn(&format!("remote cache: {reason}"));
                None
            }
        }
    }

    /// Start publishing the local entry stored under `key`.
    pub(super) fn upload(&self, key: &str, node: &TaskNode, warn: impl Fn(&str)) {
        let Some(remote) = self.remote else {
            return;
        };
        let Some(stored) = self.local.lookup(key) else {
            return;
        };
        let project = self.local.project_rel(&node.project);
        let task = TaskIdentity { project: &project, task: &node.task_name };
        if let Err(reason) = remote.upload(key, &task, &stored) {
            warn(&format!("remote cache: {reason}"));
        }
    }
}

/// The remote tier, unless `--no-cache` is set or `remoteCache` configures
/// none. One that cannot be opened is reported through `warn`, and the run
/// goes on with the local tier alone.
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
