use miette::{Result, WrapErr};
use pnpm_config::Config;
use pnpm_env_installer::ConfigDependencyStoreIndex;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_store_dir::{StoreDir, StoreIndex, StoreIndexWriter, StoreOperationLock};
use std::sync::Arc;

pub(super) struct StoreIndexSession {
    writer: Arc<StoreIndexWriter>,
    writer_task: tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>,
}

impl StoreIndexSession {
    pub(super) async fn attach(
        index: &mut ConfigDependencyStoreIndex,
        store_dir: &'static StoreDir,
        config: &Config,
    ) -> Self {
        index.reader = StoreIndex::open_shared(store_dir, config.frozen_store).await;
        let (writer, writer_task) =
            StoreIndexWriter::spawn_for(store_dir, config.frozen_store, config.store_index_format);
        index.writer = Some(Arc::clone(&writer));
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

pub(super) fn lock_config_dependency_store<Reporter: self::Reporter>(
    config: &Config,
) -> Result<StoreOperationLock> {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Debug,
        message: "Waiting for the configuration dependency store operation lock".to_string(),
    }));
    let store_lock = if config.frozen_store {
        config.store_dir.lock_for_frozen_use()
    } else {
        config.store_dir.lock_for_use()
    }
    .wrap_err("lock the store while installing configuration dependencies")?;
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Debug,
        message: "Acquired the configuration dependency store operation lock".to_string(),
    }));

    Ok(store_lock)
}
