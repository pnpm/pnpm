use super::UpstreamTarball;
use crate::server::{
    AppState, CanonicalPackageName, Integrity, expected_tarball_dist, tar_decode_limit,
    tar_integrity_within,
};
use std::{
    collections::HashSet,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

/// Concurrent and queued background `.tgz` scans after an upstream tarball is
/// promoted to cache. Separate from `state.inner.proxy.tar_decodes` so
/// background metadata derivation never delays foreground decoded-tar responses.
pub(in crate::server) struct BackgroundTarScans {
    permits: tokio::sync::Semaphore,
    state: Mutex<BackgroundScanState>,
}

#[derive(Default)]
struct BackgroundScanState {
    in_flight: HashSet<(String, String)>,
    deferred_files: HashSet<(String, String, String)>,
}

const MAX_CONCURRENT_BACKGROUND_SCANS: usize = 8;
const MAX_IN_FLIGHT_BACKGROUND_SCANS: usize = 64;
const MAX_DEFERRED_BACKGROUND_SCANS: usize = 1024;

/// Decoded-byte budget for a background `.tgz` scan when the upstream version
/// omits `dist.unpackedSize`.
const MAX_BACKGROUND_UNDECLARED_TAR_BYTES: u64 = 256 * 1024 * 1024;

struct InFlightScanGuard {
    scans: Arc<BackgroundTarScans>,
    key: Option<(String, String)>,
}

impl Drop for InFlightScanGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.scans.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .in_flight
                .remove(&key);
        }
    }
}

impl BackgroundTarScans {
    pub(in crate::server) fn new() -> Arc<Self> {
        Arc::new(Self {
            permits: tokio::sync::Semaphore::new(MAX_CONCURRENT_BACKGROUND_SCANS),
            state: Mutex::new(BackgroundScanState::default()),
        })
    }

    fn try_claim(
        self: &Arc<Self>,
        name: &CanonicalPackageName,
        tgz_key: &str,
        deferred_file: Option<(&str, &str)>,
    ) -> Option<InFlightScanGuard> {
        let key = (name.as_str().to_string(), tgz_key.to_string());
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.in_flight.contains(&key) {
                return None;
            }
            if state.in_flight.len() >= MAX_IN_FLIGHT_BACKGROUND_SCANS {
                if let Some((namespace, filename)) = deferred_file
                    && state.deferred_files.len() < MAX_DEFERRED_BACKGROUND_SCANS
                {
                    state.deferred_files.insert((
                        namespace.to_string(),
                        name.as_str().to_string(),
                        filename.to_string(),
                    ));
                }
                return None;
            }
            if let Some((namespace, filename)) = deferred_file {
                state.deferred_files.remove(&(
                    namespace.to_string(),
                    name.as_str().to_string(),
                    filename.to_string(),
                ));
            }
            state.in_flight.insert(key.clone());
        }
        Some(InFlightScanGuard { scans: Arc::clone(self), key: Some(key) })
    }

    fn take_deferred_file(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.deferred_files.is_empty()
            || state.in_flight.len() >= MAX_IN_FLIGHT_BACKGROUND_SCANS
        {
            return false;
        }
        state.deferred_files.remove(&(
            namespace.to_string(),
            name.as_str().to_string(),
            filename.to_string(),
        ))
    }
}

pub(super) struct PromotedTarballScan {
    storage: pnpr_storage::Storage,
    background_scans: Arc<BackgroundTarScans>,
    name: CanonicalPackageName,
    tgz_integrity: Integrity,
    tgz_key: String,
    namespace: String,
    filename: String,
    decode_limit: u64,
}

impl PromotedTarballScan {
    pub(super) fn new(state: &AppState, tarball: UpstreamTarball<'_>) -> Self {
        let tgz_integrity = tarball.integrity.clone();
        let tgz_key = tgz_integrity.to_string();
        Self {
            storage: state.inner.storage.clone(),
            background_scans: Arc::clone(&state.inner.proxy.background_tar_scans),
            name: tarball.name.clone(),
            tgz_integrity,
            tgz_key,
            namespace: tarball.namespace.to_string(),
            filename: tarball.filename.to_string(),
            decode_limit: background_tar_decode_limit(tarball.unpacked_size),
        }
    }

    pub(super) fn spawn(self, cached_path: PathBuf) {
        let Some(in_flight_guard) = self.background_scans.try_claim(
            &self.name,
            &self.tgz_key,
            Some((&self.namespace, &self.filename)),
        ) else {
            return;
        };
        tokio::spawn(record_cached_tgz_tar_integrity(
            self.storage,
            self.name,
            self.tgz_integrity,
            self.tgz_key,
            cached_path,
            self.decode_limit,
            in_flight_guard,
        ));
    }
}

fn background_tar_decode_limit(unpacked_size: Option<u64>) -> u64 {
    match unpacked_size {
        Some(size) => tar_decode_limit(Some(size)),
        None => MAX_BACKGROUND_UNDECLARED_TAR_BYTES,
    }
}

pub(super) fn schedule_cached_tarball_scan(state: &AppState, tarball: UpstreamTarball<'_>) {
    let tgz_key = tarball.integrity.to_string();
    if state.inner.storage.has_cached_scanned_tar_integrity(tarball.name, &tgz_key) {
        return;
    }
    let Some(in_flight_guard) = state.inner.proxy.background_tar_scans.try_claim(
        tarball.name,
        &tgz_key,
        Some((tarball.namespace, tarball.filename)),
    ) else {
        return;
    };
    let cached_path =
        state.inner.storage.upstream_blob_path(tarball.namespace, tarball.name, tarball.filename);
    tokio::spawn(record_cached_tgz_tar_integrity(
        state.inner.storage.clone(),
        tarball.name.clone(),
        tarball.integrity.clone(),
        tgz_key,
        cached_path,
        background_tar_decode_limit(tarball.unpacked_size),
        in_flight_guard,
    ));
}

pub(super) fn schedule_deferred_cached_tarball_scan(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    filename: &str,
) {
    if !state.inner.proxy.background_tar_scans.take_deferred_file(namespace, name, filename) {
        return;
    }
    let storage = state.inner.storage.clone();
    let background_scans = Arc::clone(&state.inner.proxy.background_tar_scans);
    let namespace = namespace.to_string();
    let name = name.clone();
    let filename = filename.to_string();
    tokio::spawn(async move {
        let Ok(Some(packument)) = storage.read_upstream_document_any(&namespace, &name).await
        else {
            return;
        };
        let Ok(Some(dist)) = expected_tarball_dist(&packument, &name, &filename) else {
            return;
        };
        let tgz_key = dist.integrity.to_string();
        if storage.has_cached_scanned_tar_integrity(&name, &tgz_key) {
            return;
        }
        let Some(in_flight_guard) =
            background_scans.try_claim(&name, &tgz_key, Some((&namespace, &filename)))
        else {
            return;
        };
        let cached_path = storage.upstream_blob_path(&namespace, &name, &filename);
        record_cached_tgz_tar_integrity(
            storage,
            name,
            dist.integrity,
            tgz_key,
            cached_path,
            background_tar_decode_limit(dist.unpacked_size),
            in_flight_guard,
        )
        .await;
    });
}

struct VerifiedTgzReader<Reader> {
    inner: Reader,
    checker: ssri::IntegrityChecker,
}

impl<Reader: Read> Read for VerifiedTgzReader<Reader> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.inner.read(buf)?;
        if len > 0 {
            self.checker.input(&buf[..len]);
        }
        Ok(len)
    }
}

fn verified_tar_integrity_within(
    cached_path: &Path,
    tgz_integrity: Integrity,
    decode_limit: u64,
) -> Option<(Integrity, pnpm_tarball::TarInspection)> {
    let file = std::fs::File::open(cached_path).ok()?;
    let mut verified = VerifiedTgzReader {
        inner: BufReader::new(file),
        checker: ssri::IntegrityChecker::new(tgz_integrity),
    };
    let derived = tar_integrity_within(&mut verified, decode_limit)?;
    let mut tail = [0u8; 8 * 1024];
    loop {
        match Read::read(&mut verified, &mut tail) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    verified.checker.result().ok()?;
    Some(derived)
}

async fn record_cached_tgz_tar_integrity(
    storage: pnpr_storage::Storage,
    name: CanonicalPackageName,
    tgz_integrity: Integrity,
    tgz_key: String,
    cached_path: PathBuf,
    decode_limit: u64,
    in_flight_guard: InFlightScanGuard,
) {
    let permit = in_flight_guard.scans.permits
        .acquire()
        .await
        .expect("the background scan semaphore is never closed");
    let existing = storage
        .read_tar_integrities(&name)
        .await
        .ok()
        .and_then(|table| table.get(&tgz_key).cloned());
    if existing.as_ref().is_some_and(pnpr_storage::DerivedTar::is_fully_scanned) {
        return;
    }
    let derived = tokio::task::spawn_blocking(move || {
        verified_tar_integrity_within(&cached_path, tgz_integrity, decode_limit)
    })
    .await
    .ok()
    .flatten();
    drop(permit);
    let derived = match (derived, existing) {
        (Some((tar_integrity, inspection)), _) => Some(
            pnpr_storage::DerivedTar::new(
                tar_integrity.to_string(),
                inspection.requires_build,
                inspection.file_count,
                inspection.unpacked_size,
            )
            .with_bins(inspection.bins),
        ),
        (None, Some(existing)) if existing.scanned && existing.bins.is_none() => {
            Some(existing.with_bins(Some(std::collections::BTreeMap::new())))
        }
        (None, _) => None,
    };
    if let Some(derived) = derived
        && let Err(err) = storage.record_tar_integrity(&name, &tgz_key, derived).await
    {
        tracing::warn!(?err, package = %name.as_str(), "recording a tar integrity failed");
    }
}
