use super::{
    EngineCallGuard, EngineMode, InstallOptions, LogSink, NativeRenderer, OutputSink,
    build_renderer, engine_call_lock, napi, run_install_inner,
};
use pnpm_package_manager::RebuildOptions;

#[napi]
pub async fn rebuild(
    options: InstallOptions,
    on_log: Option<LogSink>,
    selected_names: Option<Vec<String>>,
    on_output: Option<OutputSink>,
) -> napi::Result<()> {
    pnpm_package_manager::configure_rayon_pool();
    let _guard = engine_call_lock().lock().await;
    let renderer = build_renderer(&options, on_output);
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pnpm-napi-rebuild".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(run_rebuild_blocking(&options, on_log, renderer, selected_names));
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
    let rebuild_options =
        rebuild_options(selected_names, options.skip_if_has_side_effects_cache.unwrap_or(false));
    let outcome = run_install_inner(options, None, EngineMode::Rebuild(rebuild_options));
    outcome.map(|_| ())
}

/// The rebuild the engine API runs.
pub(super) fn rebuild_options(
    selected_names: Option<Vec<String>>,
    skip_if_has_side_effects_cache: bool,
) -> RebuildOptions {
    // `None` (or an empty list) rebuilds every build-needing package; a
    // non-empty list restricts the rebuild to the matching names / build keys.
    RebuildOptions {
        selected_names: selected_names
            .filter(|names| !names.is_empty())
            .map(|names| names.into_iter().collect()),
        // The engine API rebuilds dependencies only; running a workspace
        // project's own deferred scripts is `pnpm rebuild --pending`.
        pending_projects: Vec::new(),
        // The embedder rebuilds the lockfile it just installed or restored,
        // so of the settings it records only the patches, which change the
        // build output, have to match the configuration — as in pnpm v11's
        // `rebuild`. Comparing the rest, like `pnpmfileChecksum`, fails a
        // rebuild of a lockfile resolved with other `readPackage` hooks or
        // settings.
        check_lockfile_patches_only: true,
        skip_if_has_side_effects_cache,
    }
}
