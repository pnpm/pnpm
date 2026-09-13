use super::{
    EngineCallGuard, EngineMode, InstallOptions, LogSink, NativeRenderer, OutputSink,
    RebuildOptions, build_renderer, engine_call_lock, run_install_inner,
};
use napi_derive::napi;

#[napi]
pub async fn rebuild(
    options: InstallOptions,
    on_log: Option<LogSink>,
    selected_names: Option<Vec<String>>,
    on_output: Option<OutputSink>,
) -> napi::Result<()> {
    let _guard = engine_call_lock().lock().await;
    let renderer = build_renderer(&options, on_output);
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pnpm-napi-rebuild".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(run_rebuild_blocking(
                &options,
                on_log,
                renderer,
                selected_names,
            ));
        })
        .map_err(|error| {
            napi::Error::from_reason(format!("failed to spawn rebuild thread: {error}"))
        })?;
    rx.await.map_err(|_| napi::Error::from_reason("rebuild worker thread panicked"))?
}

fn run_rebuild_blocking(
    options: &InstallOptions,
    on_log: Option<LogSink>,
    renderer: Option<NativeRenderer>,
    selected_names: Option<Vec<String>>,
) -> napi::Result<()> {
    // Restores the previous sink and renderer on drop — including on a
    // panic in `run_install_inner`, which unwinds this dedicated thread.
    let _sink_guard = EngineCallGuard::with_renderer(on_log, renderer);
    // `None` (or an empty list) rebuilds every build-needing package; a
    // non-empty list restricts the rebuild to the matching names / build keys.
    let rebuild_options = RebuildOptions {
        selected_names: selected_names
            .filter(|names| !names.is_empty())
            .map(|names| names.into_iter().collect()),
        // The engine API rebuilds dependencies only; running a workspace
        // project's own deferred scripts is `pnpm rebuild --pending`.
        pending_projects: Vec::new(),
    };
    let outcome = run_install_inner(options, None, EngineMode::Rebuild(rebuild_options));
    outcome.map(|_| ())
}
