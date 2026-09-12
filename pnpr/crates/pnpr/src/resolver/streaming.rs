use super::{
    Arc, AuthHeaders, CachedResolution, Duration, Footprint, HashMap, Lockfile, Mutex, OsvIndex,
    PackageVersionGuard, PacquetConfig, ResolveRequest, Resolver, Response, StoreCandidate,
    StreamObserver, TarballRouter, ThrottledClient, done_frame, error_frame,
    ndjson_stream_response, osv_violations_for_lockfile, resolve, store_resolution_candidate,
    violations_frame,
};

/// What one streamed resolve carries out of the request handling.
pub(super) struct StreamedResolveInputs {
    pub(super) config: &'static PacquetConfig,
    pub(super) request: ResolveRequest,
    pub(super) request_auth: Arc<AuthHeaders>,
    pub(super) tarball_router: TarballRouter,
    pub(super) footprint: Arc<Mutex<Footprint>>,
    pub(super) cache_key: Option<String>,
}

/// Streaming resolve. Run it in a detached task that pushes one
/// `package` frame per resolved tarball into the channel via the
/// observer, then a terminal `done` / `error` frame. The response
/// body drains the channel as frames arrive.
pub(super) fn stream_resolve_response(
    runtime: &Resolver,
    inputs: StreamedResolveInputs,
) -> Response {
    let package_version_guard =
        runtime.osv_index.as_ref().map(|index| Arc::clone(index) as Arc<dyn PackageVersionGuard>);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let observer: Arc<dyn pnpm_package_manager::ResolutionObserver> = Arc::new(StreamObserver {
        tx: tx.clone(),
        package_version_guard,
        tarball_router: inputs.tarball_router.clone(),
    });
    tokio::spawn(stream_resolution(StreamedResolve {
        config: inputs.config,
        client: Arc::clone(&runtime.client),
        request: inputs.request,
        request_auth: inputs.request_auth,
        observer,
        tarball_router: inputs.tarball_router,
        osv_index: runtime.osv_index.clone(),
        cache: Arc::clone(&runtime.resolution_cache),
        cache_ttl: runtime.resolution_cache_ttl,
        cache_key: inputs.cache_key,
        cache_secret: Arc::clone(&runtime.resolution_cache_secret),
        footprint: inputs.footprint,
        tx,
    }));
    ndjson_stream_response(rx)
}

/// Everything the detached resolve task carries away from the request.
pub(super) struct StreamedResolve {
    pub(super) config: &'static PacquetConfig,
    pub(super) client: Arc<ThrottledClient>,
    pub(super) request: ResolveRequest,
    pub(super) request_auth: Arc<AuthHeaders>,
    pub(super) observer: Arc<dyn pnpm_package_manager::ResolutionObserver>,
    pub(super) tarball_router: TarballRouter,
    pub(super) osv_index: Option<Arc<OsvIndex>>,
    pub(super) cache: Arc<Mutex<HashMap<String, Vec<CachedResolution>>>>,
    pub(super) cache_ttl: Duration,
    pub(super) cache_key: Option<String>,
    pub(super) cache_secret: Arc<[u8]>,
    pub(super) footprint: Arc<Mutex<Footprint>>,
    pub(super) tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

/// Resolve, then send the terminal `done` / `error` frame. The `package`
/// frames reach the channel from the observer as each tarball resolves.
pub(super) async fn stream_resolution(task: StreamedResolve) {
    let StreamedResolve { config, tarball_router, tx, .. } = &task;
    let resolved = Box::pin(resolve::resolve(
        task.config,
        &task.client,
        &task.request,
        &task.request_auth,
        Some(Arc::clone(&task.observer)),
    ))
    .await;
    let lockfile = match resolved {
        Ok(lockfile) => tarball_router.route_lockfile(config, &lockfile),
        Err(err) => {
            let _ = tx.send(error_frame(&err.to_string()));
            return;
        }
    };
    if let Some(violations) = task.osv_violations(&lockfile) {
        let _ = tx.send(violations);
        return;
    }
    if let Some(key) = task.cache_key.clone() {
        store_resolution_candidate(StoreCandidate {
            cache: &task.cache,
            cache_ttl: task.cache_ttl,
            key,
            footprint: &task.footprint,
            cache_secret: &task.cache_secret,
            lockfile: &lockfile,
        });
    }
    let _ = tx.send(done_frame(&lockfile));
}

impl StreamedResolve {
    /// The violations frame a resolved lockfile earns, if the OSV index
    /// refuses any of its packages.
    pub(super) fn osv_violations(&self, lockfile: &Lockfile) -> Option<Vec<u8>> {
        let osv_index = self.osv_index.as_ref()?;
        let violations = osv_violations_for_lockfile(osv_index, lockfile);
        (!violations.is_empty()).then(|| violations_frame(&violations))
    }
}
