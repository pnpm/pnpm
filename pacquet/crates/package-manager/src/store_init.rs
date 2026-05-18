use pacquet_store_dir::StoreDir;

/// Best-effort eager CAFS-shard bootstrap at install start. Delegates
/// to [`StoreDir::init`] on the blocking pool and degrades every
/// failure mode — `JoinError` (task panic / cancellation) and
/// `io::Error` (init itself failed: permission denied, disk full,
/// non-directory at `v11/files`, ...) — to a single `warn!`. The lazy
/// per-shard fallback inside
/// [`StoreDir::write_cas_file`][pacquet_store_dir::StoreDir::write_cas_file]
/// handles whatever `init` didn't, so there's no correctness reason to
/// fail the install on a bootstrap miss.
///
/// Shared by `InstallWithoutLockfile::run` and `CreateVirtualStore::run`
/// so the log text and degradation policy stay in sync — a previous
/// pass had them drift, which made grepping the install log
/// frustrating.
pub(crate) async fn init_store_dir_best_effort(store_dir: &'static StoreDir) {
    match tokio::task::spawn_blocking(move || store_dir.init()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(
                target: "pacquet::install",
                ?error,
                "store-dir init failed; continuing — write-side lazy mkdir fallback will handle it",
            );
        }
        Err(error) => {
            tracing::warn!(
                target: "pacquet::install",
                ?error,
                "store-dir init task panicked or was cancelled; continuing — write-side lazy mkdir fallback will handle it",
            );
        }
    }
}
