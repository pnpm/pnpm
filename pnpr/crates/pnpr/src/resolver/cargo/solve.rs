use std::{collections::BTreeMap, sync::Arc};

use axum::response::Response;

use super::{
    super::{
        limits::{CancelToken, ResolvePermit, request_timeout_response},
        report_message,
        wire::{cargo_done_frame_until, error_frame, ndjson_single_frame},
    },
    IndexFetcher,
};

pub(super) struct DiscoveryStep {
    pub(super) discovery: pnpm_cargo_resolver::IndexDiscovery,
    pub(super) missing: Vec<String>,
    pub(super) entries: BTreeMap<String, String>,
}

pub(super) fn init_discovery(
    metadata: &str,
    source: &str,
    cancel: &CancelToken,
) -> Result<(pnpm_cargo_resolver::IndexDiscovery, Vec<String>), String> {
    let mut discovery = pnpm_cargo_resolver::IndexDiscovery::new(metadata, source)
        .map_err(|err| report_message(&err))?;
    let names = discovery
        .missing_names_until(&|| cancel.is_cancelled())
        .map_err(|err| report_message(&err))?;
    Ok((discovery, names))
}

pub(super) fn step_discovery(
    mut discovery: pnpm_cargo_resolver::IndexDiscovery,
    entries: BTreeMap<String, String>,
    cancel: &CancelToken,
) -> Result<DiscoveryStep, String> {
    discovery
        .add_entries_until(&entries, &|| cancel.is_cancelled())
        .map_err(|err| report_message(&err))?;
    let missing = discovery
        .missing_names_until(&|| cancel.is_cancelled())
        .map_err(|err| report_message(&err))?;
    Ok(DiscoveryStep { discovery, missing, entries })
}

pub(super) async fn spawn_init_discovery(
    metadata: String,
    source: String,
    permit: &Arc<ResolvePermit>,
    cancel: &CancelToken,
) -> Result<(pnpm_cargo_resolver::IndexDiscovery, Vec<String>), String> {
    let permit = Arc::clone(permit);
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        init_discovery(&metadata, &source, &cancel)
    })
    .await
    .map_err(|err| err.to_string())?
}

pub(super) async fn spawn_step_discovery(
    discovery: pnpm_cargo_resolver::IndexDiscovery,
    entries: BTreeMap<String, String>,
    permit: &Arc<ResolvePermit>,
    cancel: &CancelToken,
) -> Result<DiscoveryStep, String> {
    let permit = Arc::clone(permit);
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        step_discovery(discovery, entries, &cancel)
    })
    .await
    .map_err(|err| err.to_string())?
}

pub(in crate::resolver) async fn solve_cargo(
    index: IndexFetcher,
    metadata: String,
    source: String,
    preferred_versions: BTreeMap<String, Vec<String>>,
    permit: ResolvePermit,
    deadline: tokio::time::Instant,
) -> Response {
    let permit = Arc::new(permit);
    let (cancel, _cancel_on_drop) = CancelToken::new(deadline);
    let index_files = match index.fetch_for(&metadata, &permit, &cancel).await {
        Ok(index_files) => index_files,
        Err(err) => {
            return if cancel.deadline_expired() {
                request_timeout_response()
            } else {
                ndjson_single_frame(&error_frame(&err))
            };
        }
    };
    let solve_permit = Arc::clone(&permit);
    let solve_cancel = cancel.clone();
    let solved = tokio::task::spawn_blocking(move || {
        let _permit = solve_permit;
        pnpm_cargo_resolver::resolve_lockfile_until_with_preferences(
            &metadata,
            &index_files,
            &source,
            &preferred_versions,
            &|| solve_cancel.is_cancelled(),
        )
        .map(|lockfile| cargo_done_frame_until(&lockfile, &|| solve_cancel.is_cancelled()))
    })
    .await;
    match solved {
        Ok(Ok(Some(frame))) => ndjson_single_frame(&frame),
        Ok(Ok(None)) => request_timeout_response(),
        Ok(Err(err))
            if cancel.deadline_expired() || err.is::<pnpm_cargo_resolver::CancelError>() =>
        {
            request_timeout_response()
        }
        Ok(Err(err)) => ndjson_single_frame(&error_frame(&report_message(&err))),
        Err(err) => ndjson_single_frame(&error_frame(&err.to_string())),
    }
}
