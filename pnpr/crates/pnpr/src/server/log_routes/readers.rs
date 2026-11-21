use super::super::{
    AppState,
    Identity,
    Registry,
    RoutingSnapshot,
};
use pnpm_release_record::{
    LogEntry,
    RecordIdentity,
};
use pnpm_transparency_log::TILE_WIDTH;
use pnpr_config::HostedConfig;
use pnpr_registry::PackagePattern;
use pnpr_storage::transparency_log::LogId;

fn log_registries<'a>(
    routing: &'a RoutingSnapshot,
    log: &'a LogId,
) -> impl Iterator<Item = (&'a str, &'a HostedConfig, &'a [PackagePattern])> {
    routing.hosted
        .iter()
        .filter_map(move |(name, hosted)| {
            if hosted.namespace != log.namespace
                || routing.registries.ecosystem(name) != Some(log.ecosystem)
            {
                return None;
            }
            let Registry::Hosted { patterns } = routing.registries.get(name)? else { return None };
            let claims =
                if patterns.is_empty() { &[PackagePattern::All][..] } else { patterns.as_slice() };
            Some((name.as_str(), hosted, claims))
        })
}

/// What one caller may read of a log.
pub(super) struct LogReaders {
    /// The caller may read some package the log holds.
    pub(super) may_read_some: bool,
    /// The caller may read every package the log holds.
    pub(super) may_read_all: bool,
}

impl LogReaders {
    /// What `identity` may read of `log`, by the rules of every hosted
    /// registry of its namespace.
    pub(super) fn of(state: &AppState, log: &LogId, identity: &Identity) -> Self {
        let routing = state.routing();
        let mut may_read_some = false;
        let mut may_read_all = true;
        for (_, hosted, claims) in log_registries(&routing, log) {
            let rules = hosted.rules.snapshot();
            may_read_some |= rules.any_access_admits_in(identity, claims);
            may_read_all &= rules.all_access_admit_in(identity, claims);
        }
        let may_read_all = may_read_some && may_read_all;
        Self { may_read_some, may_read_all }
    }
}

/// A log retains entries after its registry narrows its claims. Check the
/// append-only prefix once per routing snapshot, across all bundles.
pub(super) async fn log_still_claimed(
    state: &AppState,
    log: &LogId,
    index: u64,
    width: u32,
) -> pnpr_error::Result<bool> {
    let routing = state.routing();
    let Some(end) = index
        .checked_mul(u64::from(TILE_WIDTH))
        .and_then(|start| start.checked_add(u64::from(width)))
    else {
        return Ok(false);
    };
    let database = state.inner.storage.sql_database();
    for (name, _, patterns) in log_registries(&routing, log) {
        if patterns.contains(&PackagePattern::All) {
            continue;
        }
        let mut prefix = routing.caches.claimed_log_prefixes[name].lock().await;
        if !check_prefix(database, log, patterns, end, &mut prefix).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn check_prefix(
    database: &std::sync::Arc<pnpr_storage::database::SqlDatabase>,
    log: &LogId,
    patterns: &[PackagePattern],
    end: u64,
    prefix: &mut Option<u64>,
) -> pnpr_error::Result<bool> {
    let Some(mut checked) = *prefix else { return Ok(false) };
    let size = database.log_size(log).await?;
    if size < end {
        return Ok(false);
    }
    while checked < size {
        let next = size.min(checked.saturating_add(u64::from(TILE_WIDTH)));
        let entries = database.log_entries(log, checked..next).await?;
        if !entries.iter().all(|entry| claimed_entry(patterns, entry)) {
            *prefix = None;
            return Ok(false);
        }
        checked = next;
        *prefix = Some(checked);
    }
    Ok(true)
}

fn claimed_entry(patterns: &[PackagePattern], bytes: &[u8]) -> bool {
    let Ok(entry) = LogEntry::parse(bytes) else { return false };
    let RecordIdentity::Package(purl) = &entry.key().identity else { return false };
    purl.npm_package()
        .is_some_and(|(name, _)| {
            patterns
                .iter()
                .any(|pattern| pattern.matches(&name))
        })
}
