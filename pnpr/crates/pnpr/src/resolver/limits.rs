use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpm_lockfile::Lockfile;
use pnpr_config::Config as RegistryConfig;
use pnpr_policy::Identity;
use tokio::{
    sync::{Notify, OwnedSemaphorePermit, Semaphore},
    time::Instant,
};

use crate::server::{KeyedPermit, KeyedSemaphores};

use super::{
    json_error, json_error_with_code,
    protocol::{ResolveRequest, ResolveRequestProject},
};

pub(super) const STREAM_CHANNEL_CAPACITY: usize = 256;
pub(super) const MAX_CONCURRENT_RESOLVES: usize = 16;
pub(super) const MAX_CONCURRENT_RESOLVES_PER_IDENTITY: usize = 4;
pub(super) const MAX_ADMITTED_RESOLVES: usize = 64;
pub(super) const MAX_ADMITTED_RESOLVES_PER_IDENTITY: usize = 8;

/// Fallback overall deadline when `http.request_timeout` is disabled (`0`).
pub(super) const MAX_RESOLVE_DURATION: Duration = Duration::from_mins(10);

pub(super) const MAX_WORKSPACE_PROJECTS: usize = 5_000;
pub(super) const MAX_DIRECT_DEPENDENCIES: usize = 50_000;
pub(super) const MAX_NPM_PACKAGES: usize = 25_000;
const MAX_NPM_SNAPSHOT_MULTIPLIER: usize = 2;

pub(super) const RESOLVE_TIMEOUT_CODE: &str = "ERR_PNPR_RESOLVE_TIMEOUT";
pub(super) const GRAPH_TOO_LARGE_CODE: &str = "ERR_PNPR_GRAPH_TOO_LARGE";

pub(super) struct ResolverLimits {
    instance_admission: Arc<Semaphore>,
    identity_admission: KeyedSemaphores,
    instance_permits: Arc<Semaphore>,
    identity_permits: KeyedSemaphores,
    pub(super) deadline: Duration,
    pub(super) max_npm_packages: usize,
}

impl ResolverLimits {
    pub(super) fn from_config(config: &RegistryConfig) -> Self {
        Self {
            instance_admission: Arc::new(Semaphore::new(MAX_ADMITTED_RESOLVES)),
            identity_admission: KeyedSemaphores::new(MAX_ADMITTED_RESOLVES_PER_IDENTITY),
            instance_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_RESOLVES)),
            identity_permits: KeyedSemaphores::new(MAX_CONCURRENT_RESOLVES_PER_IDENTITY),
            deadline: config.http.request_timeout.unwrap_or(MAX_RESOLVE_DURATION),
            max_npm_packages: MAX_NPM_PACKAGES,
        }
    }

    pub(super) fn start_deadline(&self) -> Instant {
        Instant::now() + self.deadline
    }

    /// Bound total running + queued work before touching caches or waiting for
    /// a running slot. Per-identity admission is taken first so one noisy
    /// caller cannot exhaust the instance queue.
    pub(super) fn admit<'a>(
        &'a self,
        identity: &Identity,
    ) -> Result<AdmittedResolve<'a>, Response> {
        let key = identity.user_id().unwrap_or("$anonymous");
        let Some(admitted_identity) = self.identity_admission.try_acquire(key) else {
            return Err(too_many_requests_response());
        };
        let Ok(admitted_instance) = Arc::clone(&self.instance_admission).try_acquire_owned() else {
            return Err(too_many_requests_response());
        };
        Ok(AdmittedResolve {
            limits: self,
            key: key.to_string(),
            admitted_identity,
            admitted_instance,
        })
    }

    #[cfg(test)]
    pub(super) fn set_concurrency(&mut self, max_instance: usize, max_per_identity: usize) {
        self.instance_admission = Arc::new(Semaphore::new(max_instance.saturating_mul(4)));
        self.identity_admission = KeyedSemaphores::new(max_per_identity.saturating_mul(2));
        self.instance_permits = Arc::new(Semaphore::new(max_instance));
        self.identity_permits = KeyedSemaphores::new(max_per_identity);
    }

    #[cfg(test)]
    pub(super) fn set_admission(&mut self, max_instance: usize, max_per_identity: usize) {
        self.instance_admission = Arc::new(Semaphore::new(max_instance));
        self.identity_admission = KeyedSemaphores::new(max_per_identity);
    }

    #[cfg(test)]
    pub(super) fn available_instance_permits(&self) -> usize {
        self.instance_permits.available_permits()
    }

    #[cfg(test)]
    pub(super) fn tracked_identities(&self) -> usize {
        self.identity_permits.tracked_keys().max(self.identity_admission.tracked_keys())
    }
}

pub(super) struct AdmittedResolve<'a> {
    limits: &'a ResolverLimits,
    key: String,
    admitted_identity: KeyedPermit,
    admitted_instance: OwnedSemaphorePermit,
}

impl AdmittedResolve<'_> {
    /// Take the caller's running slot before an instance slot so noisy callers
    /// queue behind themselves instead of starving other identities.
    pub(super) async fn acquire(self, deadline: Instant) -> Result<ResolvePermit, Response> {
        if Instant::now() >= deadline {
            return Err(request_timeout_response());
        }
        let Self {
            limits,
            key,
            admitted_identity,
            admitted_instance,
        } = self;
        tokio::time::timeout_at(deadline, async move {
            let active_identity = limits.identity_permits.acquire(&key).await;
            let active_instance = Arc::clone(&limits.instance_permits)
                .acquire_owned()
                .await
                .expect("resolver semaphore is never closed");
            ResolvePermit {
                _admitted_identity: admitted_identity,
                _admitted_instance: admitted_instance,
                _active_identity: active_identity,
                _active_instance: active_instance,
            }
        })
        .await
        .map_err(|_| request_timeout_response())
    }
}

#[derive(Clone)]
pub(super) struct CancelToken {
    cancelled: Arc<AtomicBool>,
    deadline: std::time::Instant,
}

impl CancelToken {
    pub(super) fn new(deadline: Instant) -> (Self, CancelOnDrop) {
        let cancelled = Arc::new(AtomicBool::new(false));
        let guard = CancelOnDrop(Arc::clone(&cancelled));
        (Self { cancelled, deadline: deadline.into_std() }, guard)
    }

    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub(super) fn deadline_expired(&self) -> bool {
        std::time::Instant::now() >= self.deadline
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || self.deadline_expired()
    }
}

pub(super) struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(super) struct ResolvePermit {
    _admitted_identity: KeyedPermit,
    _admitted_instance: OwnedSemaphorePermit,
    _active_identity: KeyedPermit,
    _active_instance: OwnedSemaphorePermit,
}

/// Tracks live package resolutions during a streaming tree walk so an
/// oversized graph aborts `resolve::resolve` immediately, even when edges are
/// `optionalDependencies` whose individual resolver errors would be dropped.
pub(super) struct GraphLimitTracker {
    max_packages: usize,
    resolved_count: AtomicUsize,
    exceeded: AtomicBool,
    notify: Notify,
}

impl GraphLimitTracker {
    pub(super) fn new(max_packages: usize) -> Arc<Self> {
        Arc::new(Self {
            max_packages,
            resolved_count: AtomicUsize::new(0),
            exceeded: AtomicBool::new(false),
            notify: Notify::new(),
        })
    }

    pub(super) fn record_package(&self) -> bool {
        let next = self.resolved_count.fetch_add(1, Ordering::Relaxed) + 1;
        if next <= self.max_packages {
            return true;
        }
        if !self.exceeded.swap(true, Ordering::AcqRel) {
            self.notify.notify_one();
        }
        false
    }

    pub(super) fn is_exceeded(&self) -> bool {
        self.exceeded.load(Ordering::Acquire)
    }

    pub(super) fn notified(&self) -> tokio::sync::futures::Notified<'_> {
        self.notify.notified()
    }

    pub(super) fn error_message(&self) -> String {
        format!("dependency graph exceeds maximum of {} packages", self.max_packages)
    }

    pub(super) fn max_snapshots(&self) -> usize {
        self.max_packages.saturating_mul(MAX_NPM_SNAPSHOT_MULTIPLIER)
    }
}

fn too_many_requests_response() -> Response {
    json_error(StatusCode::SERVICE_UNAVAILABLE, "too many active resolver requests")
}

pub(super) fn request_timeout_response() -> Response {
    let mut response = (
        StatusCode::GATEWAY_TIMEOUT,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"error":"request timed out","code":"ERR_PNPR_RESOLVE_TIMEOUT"}"#,
    )
        .into_response();
    response
        .extensions_mut()
        .insert(pnpr_error::PublicErrorMessage("request timed out".to_string()));
    response
}

pub(super) fn reject_oversized_graph(
    request: &ResolveRequest,
    max_packages: usize,
) -> Option<Response> {
    let project_count = request.projects.as_ref().map_or(1, Vec::len);
    if project_count > MAX_WORKSPACE_PROJECTS {
        return Some(graph_too_large_response(&format!(
            "workspace exceeds maximum of {MAX_WORKSPACE_PROJECTS} projects",
        )));
    }
    if declared_dependency_entries(request) > MAX_DIRECT_DEPENDENCIES {
        return Some(graph_too_large_response(&format!(
            "request exceeds maximum of {MAX_DIRECT_DEPENDENCIES} dependency entries",
        )));
    }
    let lockfile = request.lockfile.as_ref()?;
    if lockfile.importers.len() > MAX_WORKSPACE_PROJECTS {
        return Some(graph_too_large_response(&format!(
            "lockfile exceeds maximum of {MAX_WORKSPACE_PROJECTS} importers",
        )));
    }
    lockfile_exceeds_graph_limit(lockfile, max_packages)
        .map(|message| graph_too_large_response(&message))
}

pub(super) fn lockfile_exceeds_graph_limit(
    lockfile: &Lockfile,
    max_packages: usize,
) -> Option<String> {
    let packages = lockfile.packages.as_ref().map_or(0, HashMap::len);
    if packages > max_packages {
        return Some(format!("dependency graph exceeds maximum of {max_packages} packages"));
    }
    let max_snapshots = max_packages.saturating_mul(MAX_NPM_SNAPSHOT_MULTIPLIER);
    let snapshots = lockfile.snapshots.as_ref().map_or(0, HashMap::len);
    (snapshots > max_snapshots).then(|| {
        format!("dependency graph exceeds maximum of {max_snapshots} package snapshots")
    })
}

fn graph_too_large_response(message: &str) -> Response {
    json_error_with_code(StatusCode::BAD_REQUEST, message, Some(GRAPH_TOO_LARGE_CODE))
}

fn project_dependency_entries(project: &ResolveRequestProject) -> usize {
    project.dependencies
        .len()
        .saturating_add(project.dev_dependencies.len())
        .saturating_add(project.optional_dependencies.len())
        .saturating_add(project.peer_dependencies.len())
}

fn optional_dep_map_len(deps: Option<&BTreeMap<String, String>>) -> usize {
    deps.map_or(0, BTreeMap::len)
}

fn declared_dependency_entries(request: &ResolveRequest) -> usize {
    let mut total = [
        request.dependencies.as_ref(),
        request.dev_dependencies.as_ref(),
        request.optional_dependencies.as_ref(),
        request.peer_dependencies.as_ref(),
    ]
    .into_iter()
    .map(optional_dep_map_len)
    .fold(0usize, usize::saturating_add);
    if let Some(projects) = request.projects.as_deref() {
        total = projects
            .iter()
            .map(project_dependency_entries)
            .fold(total, usize::saturating_add);
    }
    if let Some(catalogs) = request.catalogs.as_ref() {
        total = catalogs
            .values()
            .map(BTreeMap::len)
            .fold(total, usize::saturating_add);
    }
    if let Some(extensions) = request.package_extensions.as_ref() {
        for extension in extensions.values() {
            total = [
                extension.dependencies.as_ref(),
                extension.optional_dependencies.as_ref(),
                extension.peer_dependencies.as_ref(),
            ]
            .into_iter()
            .map(optional_dep_map_len)
            .fold(total, usize::saturating_add);
        }
    }
    total
}
