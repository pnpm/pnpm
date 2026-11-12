use tokio::time::Instant;

use super::{
    ActiveConfig, Arc, AuthHeaders, CachedResolution, Duration, Footprint, HashMap,
    KEEPALIVE_INTERVAL, Lockfile, Mutex, OsvIndex, PackageVersionGuard, ResolveRequest, Resolver,
    Response, StoreCandidate, StreamObserver, TarballRouter, ThrottledClient,
    error_frame_with_code,
    limits::{
        CancelToken, GRAPH_TOO_LARGE_CODE, GraphLimitTracker, RESOLVE_TIMEOUT_CODE, ResolvePermit,
        STREAM_CHANNEL_CAPACITY, lockfile_exceeds_graph_limit,
    },
    ndjson_stream_response, osv_violations_for_lockfile_until, progress_frame, resolve,
    store_resolution_candidate, try_done_frame_until, violations_frame,
    wire::AbortOnDrop,
};

pub(super) struct TerminalSender {
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    slot: tokio::sync::mpsc::OwnedPermit<Vec<u8>>,
}

impl TerminalSender {
    pub(super) fn reserve(tx: tokio::sync::mpsc::Sender<Vec<u8>>) -> Self {
        let slot = tx
            .clone()
            .try_reserve_owned()
            .expect("stream channel must reserve one slot for the terminal frame");
        Self { tx, slot }
    }

    async fn closed(&self) {
        self.tx.closed().await;
    }

    fn send_frame(self, frame: Vec<u8>) {
        let _ = self.slot.send(frame);
    }
}

pub(super) struct StreamedResolveInputs {
    pub(super) config: ActiveConfig,
    pub(super) request: ResolveRequest,
    pub(super) request_auth: Arc<AuthHeaders>,
    pub(super) tarball_router: TarballRouter,
    pub(super) footprint: Arc<Mutex<Footprint>>,
    pub(super) cache_key: Option<String>,
    pub(super) permit: Arc<ResolvePermit>,
    pub(super) deadline: Instant,
}

pub(super) struct StreamCacheTarget {
    pub(super) entries: Arc<Mutex<HashMap<String, Vec<CachedResolution>>>>,
    pub(super) ttl: Duration,
    pub(super) secret: Arc<[u8]>,
}

/// Spawn resolution onto a bounded frame channel whose response body aborts
/// the task (releasing permits and temp workspace) when the client disconnects.
pub(super) fn stream_resolve_response(
    runtime: &Resolver,
    inputs: StreamedResolveInputs,
) -> Response {
    let package_version_guard = runtime.osv_index
        .as_ref()
        .map(|index| Arc::clone(index) as Arc<dyn PackageVersionGuard>);
    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(STREAM_CHANNEL_CAPACITY);
    let terminal = TerminalSender::reserve(tx.clone());
    let max_npm_packages = runtime.limits.max_npm_packages;
    let graph_limit = GraphLimitTracker::new(max_npm_packages);
    let observer: Arc<dyn pnpm_package_manager::ResolutionObserver> = Arc::new(StreamObserver {
        tx: tx.clone(),
        package_version_guard,
        tarball_router: inputs.tarball_router.clone(),
        graph_limit: Arc::clone(&graph_limit),
    });
    let keepalive = tx;
    let task = StreamedResolve {
        client: Arc::clone(&runtime.client),
        observer,
        graph_limit,
        osv_index: runtime.osv_index.as_ref().map(Arc::clone),
        cache: StreamCacheTarget {
            entries: Arc::clone(&runtime.cache.entries),
            ttl: runtime.cache.ttl,
            secret: Arc::clone(&runtime.cache.secret),
        },
        terminal,
        inputs,
        max_npm_packages,
    };
    let handle = tokio::spawn(async move {
        tokio::select! {
            () = stream_resolution(task) => {},
            () = send_keepalives(&keepalive, KEEPALIVE_INTERVAL) => {},
        }
    });
    ndjson_stream_response(rx, AbortOnDrop(handle.abort_handle()))
}

/// Send a `progress` frame every `interval` until the client hangs up. Never
/// returns otherwise: the resolve task drops it when the resolution ends.
pub(super) async fn send_keepalives(tx: &tokio::sync::mpsc::Sender<Vec<u8>>, interval: Duration) {
    let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
    loop {
        tokio::select! {
            () = tx.closed() => return,
            _ = ticks.tick() => {
                if matches!(
                    tx.try_send(progress_frame()),
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)),
                ) {
                    return;
                }
            }
        }
    }
}

/// Everything the detached resolve task carries away from the request.
pub(super) struct StreamedResolve {
    pub(super) client: Arc<ThrottledClient>,
    pub(super) observer: Arc<dyn pnpm_package_manager::ResolutionObserver>,
    pub(super) graph_limit: Arc<GraphLimitTracker>,
    pub(super) osv_index: Option<Arc<OsvIndex>>,
    pub(super) cache: StreamCacheTarget,
    pub(super) terminal: TerminalSender,
    pub(super) inputs: StreamedResolveInputs,
    pub(super) max_npm_packages: usize,
}

enum BoundedResolveOutcome {
    Disconnected,
    TimedOut,
    GraphTooLarge(String),
    Completed(Box<Result<Lockfile, super::resolve::ResolveError>>),
}

async fn run_bounded_resolve(task: &StreamedResolve) -> BoundedResolveOutcome {
    let resolve_call = resolve::resolve(
        &task.inputs.config,
        &task.client,
        &task.inputs.request,
        &task.inputs.request_auth,
        Some(Arc::clone(&task.observer)),
    );
    let mut resolve_fut = std::pin::pin!(resolve_call);
    let notified = task.graph_limit.notified();
    let mut graph_exceeded = std::pin::pin!(notified);
    graph_exceeded.as_mut().enable();
    if task.graph_limit.is_exceeded() {
        return BoundedResolveOutcome::GraphTooLarge(task.graph_limit.error_message());
    }
    tokio::select! {
        biased;
        () = task.terminal.closed() => BoundedResolveOutcome::Disconnected,
        () = &mut graph_exceeded => {
            BoundedResolveOutcome::GraphTooLarge(task.graph_limit.error_message())
        }
        () = tokio::time::sleep_until(task.inputs.deadline) => BoundedResolveOutcome::TimedOut,
        resolved = &mut resolve_fut => {
            if task.graph_limit.is_exceeded() {
                BoundedResolveOutcome::GraphTooLarge(task.graph_limit.error_message())
            } else {
                BoundedResolveOutcome::Completed(Box::new(resolved))
            }
        }
    }
}

fn send_timeout_frame(terminal: TerminalSender) {
    let frame = error_frame_with_code("resolution timed out", Some(RESOLVE_TIMEOUT_CODE));
    terminal.send_frame(frame);
}

pub(super) fn send_before_deadline(terminal: TerminalSender, deadline: Instant, frame: Vec<u8>) {
    if Instant::now() < deadline {
        terminal.send_frame(frame);
    } else {
        send_timeout_frame(terminal);
    }
}

impl StreamedResolve {
    fn into_finalizer(self) -> (TerminalSender, Instant, ResolutionFinalizer) {
        let Self {
            client: _,
            observer,
            graph_limit: _,
            osv_index,
            cache,
            terminal,
            inputs,
            max_npm_packages,
        } = self;
        drop(observer);
        let deadline = inputs.deadline;
        let finalizer = ResolutionFinalizer { osv_index, cache, inputs, max_npm_packages };
        (terminal, deadline, finalizer)
    }
}

pub(super) async fn finish_before_deadline(
    task: StreamedResolve,
    resolved: Result<Lockfile, super::resolve::ResolveError>,
) {
    let (terminal, deadline, finalizer) = task.into_finalizer();
    let (cancel, _cancel_on_drop) = CancelToken::new(deadline);
    let worker_cancel = cancel.clone();
    let mut finishing =
        tokio::task::spawn_blocking(move || finalizer.finish(resolved, &worker_cancel));
    tokio::select! {
        biased;
        () = terminal.closed() => cancel.cancel(),
        () = tokio::time::sleep_until(deadline) => {
            cancel.cancel();
            send_timeout_frame(terminal);
        }
        joined = &mut finishing => match joined {
            Ok(Some(frame)) => send_before_deadline(terminal, deadline, frame),
            Ok(None) => send_timeout_frame(terminal),
            Err(err) => {
                send_before_deadline(terminal, deadline, error_frame_with_code(&err.to_string(), None));
            }
        },
    }
}

pub(super) async fn stream_resolution(task: StreamedResolve) {
    let deadline = task.inputs.deadline;
    let resolved = match run_bounded_resolve(&task).await {
        BoundedResolveOutcome::Disconnected => return,
        BoundedResolveOutcome::TimedOut => {
            send_timeout_frame(task.terminal);
            return;
        }
        BoundedResolveOutcome::GraphTooLarge(message) => {
            send_before_deadline(
                task.terminal,
                deadline,
                error_frame_with_code(&message, Some(GRAPH_TOO_LARGE_CODE)),
            );
            return;
        }
        BoundedResolveOutcome::Completed(resolved) => *resolved,
    };
    finish_before_deadline(task, resolved).await;
}

pub(super) struct ResolutionFinalizer {
    pub(super) osv_index: Option<Arc<OsvIndex>>,
    pub(super) cache: StreamCacheTarget,
    pub(super) inputs: StreamedResolveInputs,
    pub(super) max_npm_packages: usize,
}

impl ResolutionFinalizer {
    pub(super) fn finish(
        &self,
        resolved: Result<Lockfile, super::resolve::ResolveError>,
        cancel: &CancelToken,
    ) -> Option<Vec<u8>> {
        if cancel.is_cancelled() {
            return None;
        }
        let lockfile = match resolved {
            Ok(lockfile) => lockfile,
            Err(err) => {
                let code = match &err {
                    super::resolve::ResolveError::Install { code, .. } => code.as_deref(),
                    _ => None,
                };
                return Some(error_frame_with_code(&err.to_string(), code));
            }
        };
        if let Some(message) = lockfile_exceeds_graph_limit(&lockfile, self.max_npm_packages) {
            return Some(error_frame_with_code(&message, Some(GRAPH_TOO_LARGE_CODE)));
        }
        let check_cancel = || cancel.is_cancelled();
        let lockfile = self.inputs.tarball_router.route_lockfile_until(
            &self.inputs.config,
            &lockfile,
            &check_cancel,
        )?;
        if let Some(violations) = self.osv_violations(&lockfile, &check_cancel)? {
            return Some(violations);
        }
        let applied_policies = super::protocol::AppliedPolicies::from_request(&self.inputs.request);
        let done = match try_done_frame_until(&lockfile, &applied_policies, &check_cancel)? {
            Ok(done) => done,
            Err(error_frame) => return Some(error_frame),
        };
        if !cancel.is_cancelled()
            && let Some(key) = self.inputs.cache_key.clone()
        {
            self.store_candidate(key, &lockfile);
        }
        Some(done)
    }

    fn osv_violations(
        &self,
        lockfile: &Lockfile,
        should_cancel: &dyn Fn() -> bool,
    ) -> Option<Option<Vec<u8>>> {
        let Some(osv_index) = self.osv_index.as_ref() else {
            return Some(None);
        };
        let violations = osv_violations_for_lockfile_until(osv_index, lockfile, should_cancel)?;
        Some((!violations.is_empty()).then(|| violations_frame(&violations)))
    }

    fn store_candidate(&self, key: String, lockfile: &Lockfile) {
        store_resolution_candidate(StoreCandidate {
            cache: &self.cache.entries,
            cache_ttl: self.cache.ttl,
            key,
            footprint: &self.inputs.footprint,
            cache_secret: &self.cache.secret,
            lockfile,
        });
    }
}
