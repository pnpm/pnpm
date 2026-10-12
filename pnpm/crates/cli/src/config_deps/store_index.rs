use pnpm_config::Config;
use pnpm_env_installer::ConfigDepsInstallOptions;
use pnpm_store_dir::{StoreDir, StoreIndex, StoreIndexWriter};
use std::sync::Arc;

pub(super) struct StoreIndexSession {
    writer: Arc<StoreIndexWriter>,
    writer_task: tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>,
}

impl StoreIndexSession {
    pub(super) async fn attach(
        options: &mut ConfigDepsInstallOptions<'_>,
        store_dir: &'static StoreDir,
        config: &Config,
    ) -> Self {
        options.store_index = StoreIndex::open_shared(store_dir, config.frozen_store).await;
        let (writer, writer_task) =
            StoreIndexWriter::spawn_for(store_dir, config.frozen_store, config.store_index_format);
        options.store_index_writer = Some(Arc::clone(&writer));
        Self { writer, writer_task }
    }

    pub(super) async fn drain(self) {
        drop(self.writer);
        StoreIndexWriter::drain(
            self.writer_task,
            "; some config-dependency rows may not be persisted",
        )
        .await;
    }
}
