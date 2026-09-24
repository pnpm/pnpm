//! Confirm that the registry serves an exact published version and its tarball.

pub use error::PublishWaitError;

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use futures_util::{StreamExt, stream};
use pnpm_reporter::Reporter;

use crate::{
    global_log::global_info,
    publish_packed_pkg::{PublishNetwork, clean_version},
    registry_config_keys::NormalizedRegistryUrl,
};

use probe::{ProbeResult, probe_package};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const CHECK_CONCURRENCY: usize = 4;

/// Read fresh install metadata and check each exact version's tarball with HEAD.
/// The timeout covers the entire group, including requests and retry
/// delays. A zero timeout makes no requests. No install scripts are executed.
pub async fn wait_for_published_packages<Reporter: self::Reporter>(
    packages: &[(&str, &str)],
    registry: &NormalizedRegistryUrl,
    network: &PublishNetwork<'_>,
    timeout: Duration,
) -> Result<(), PublishWaitError> {
    if timeout.is_zero() || packages.is_empty() {
        return Ok(());
    }
    let packages = normalize_versions(packages)?;
    let mut pending: BTreeMap<_, _> = packages
        .iter()
        .enumerate()
        .map(|(index, (name, version))| (index, format!("{name}@{version}")))
        .collect();
    global_info::<Reporter>(&format!(
        "Waiting up to {}ms for {} package(s) to become available from {}",
        timeout.as_millis(),
        packages.len(),
        pnpm_network::redact_url_for_display(registry.as_str()),
    ));
    let deadline = Instant::now() + timeout;
    tokio::time::timeout(
        timeout,
        poll_packages(&packages, &mut pending, registry, network, deadline, timeout),
    )
    .await
    .unwrap_or_else(|_| Err(PublishWaitError::timeout(pending.into_values(), registry, timeout)))
}

async fn poll_packages(
    packages: &[(String, String)],
    pending: &mut BTreeMap<usize, String>,
    registry: &NormalizedRegistryUrl,
    network: &PublishNetwork<'_>,
    deadline: Instant,
    timeout: Duration,
) -> Result<(), PublishWaitError> {
    while !pending.is_empty() {
        if Instant::now() >= deadline {
            return Err(PublishWaitError::timeout(pending.values().cloned(), registry, timeout));
        }
        let delay = probe_pending(packages, pending, registry, network).await?.max(POLL_INTERVAL);
        if pending.is_empty() {
            break;
        }
        let now = Instant::now();
        if now
            .checked_add(delay)
            .is_none_or(|next| next >= deadline)
        {
            return Err(PublishWaitError::timeout(pending.values().cloned(), registry, timeout));
        }
        tokio::time::sleep(delay.min(deadline.saturating_duration_since(now))).await;
    }
    Ok(())
}

fn normalize_versions(
    packages: &[(&str, &str)],
) -> Result<Vec<(String, String)>, PublishWaitError> {
    packages
        .iter()
        .map(|&(name, version)| {
            clean_version(version)
                .map(|version| (name.to_owned(), version))
                .map_err(|error| PublishWaitError::check(name, version, error.to_string()))
        })
        .collect()
}

async fn probe_pending(
    packages: &[(String, String)],
    pending: &mut BTreeMap<usize, String>,
    registry: &NormalizedRegistryUrl,
    network: &PublishNetwork<'_>,
) -> Result<Duration, PublishWaitError> {
    let indexes = pending
        .keys()
        .copied()
        .collect::<Vec<_>>();
    let results = stream::iter(indexes)
        .map(|index| async move {
            let (name, version) = &packages[index];
            probe_package(name, version, registry, network).await
                .map(|result| (index, result))
                .map_err(|reason| PublishWaitError::check(name, version, reason))
        })
        .buffer_unordered(CHECK_CONCURRENCY);
    tokio::pin!(results);
    let mut delay = Duration::ZERO;
    while let Some(result) = results.next().await {
        let (index, result) = result?;
        match result {
            ProbeResult::Ready => {
                pending.remove(&index);
            }
            ProbeResult::Pending(retry_after) => {
                delay = delay.max(retry_after);
            }
        }
    }
    Ok(delay)
}

mod error;
mod probe;
#[cfg(test)]
mod tests;
