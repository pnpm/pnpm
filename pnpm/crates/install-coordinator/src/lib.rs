#[cfg_attr(target_os = "wasi", path = "metadata_file_wasi.rs")]
mod metadata_file;
mod mutation;

use futures_util::future::{BoxFuture, join_all};
use miette::Result;
use mutation::MetadataMutation;
use std::{future::Future, path::PathBuf};

/// An install projection ready to publish after all participants have settled.
/// Dropping an unpublished result must clean up its temporary resources.
pub trait PreparedInstall: Send {
    /// Publish this projection. Declared metadata files are restored by the plan.
    fn publish(&mut self) -> Result<()>;

    /// Undo non-metadata publication, including a partially failed publish.
    /// Must leave resources needed by the restored projection alive.
    fn rollback(&mut self) -> Result<()>;

    /// Keep resources referenced by published state. Also called when rollback
    /// fails, so recovery never encounters resources deleted by a destructor.
    fn retain(self: Box<Self>);
}

type Prepared = Vec<Box<dyn PreparedInstall>>;

/// A participant's metadata footprint and deferred preparation.
/// Preparation may populate caches and declared metadata, but must not publish
/// a projection that needs rollback. Spawned work must settle before returning.
#[must_use]
pub struct InstallTask<'a> {
    metadata: Vec<PathBuf>,
    prepare: BoxFuture<'a, Result<Prepared>>,
}

impl<'a> InstallTask<'a> {
    pub fn new<Prepare, Projection>(metadata: Vec<PathBuf>, prepare: Prepare) -> Self
    where
        Prepare: Future<Output = Result<Vec<Projection>>> + Send + 'a,
        Projection: PreparedInstall + 'static,
    {
        Self {
            metadata,
            prepare: Box::pin(async move {
                Ok(prepare.await?
                    .into_iter()
                    .map(|projection| Box::new(projection) as Box<dyn PreparedInstall>)
                    .collect())
            }),
        }
    }

    /// Enroll an installer with its own materialization/failure semantics.
    /// Only its declared metadata participates in this plan's rollback.
    pub fn in_place<Install>(metadata: Vec<PathBuf>, install: Install) -> Self
    where
        Install: Future<Output = Result<()>> + Send + 'a,
    {
        Self {
            metadata,
            prepare: Box::pin(async move {
                install.await?;
                Ok(Vec::new())
            }),
        }
    }
}

/// Owns one workspace lock, metadata rollback, and the publication barrier.
/// Native dependency graphs, lockfile formats and projections remain with each
/// participant. This is an in-process transaction, not a crash-recovery journal.
#[must_use]
pub struct InstallPlan<'a> {
    transaction_root: PathBuf,
    tasks: Vec<InstallTask<'a>>,
}

impl<'a> InstallPlan<'a> {
    pub fn new(transaction_root: PathBuf) -> Self {
        Self { transaction_root, tasks: Vec::new() }
    }

    pub fn with_task(mut self, task: InstallTask<'a>) -> Self {
        self.tasks.push(task);
        self
    }

    pub async fn run(self) -> Result<()> {
        let (metadata, preparations): (Vec<_>, Vec<_>) = self.tasks
            .into_iter()
            .map(|task| (task.metadata, task.prepare))
            .unzip();
        let mutation =
            MetadataMutation::capture(self.transaction_root, metadata.into_iter().flatten()).await?;
        let results = settle_all(preparations).await;
        let outcome = results
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .and_then(|prepared| publish(prepared.into_iter().flatten().collect()));
        mutation.finish(outcome)
    }
}

/// Settle every preparation concurrently.
///
/// A participant may block its task: the cold download drain does, with
/// `block_in_place`. Polled together by `join_all`, the other participants
/// would then go unpolled while they hold a shared resource the blocked one
/// waits for, such as the network budget. On the multi-thread runtime, each
/// participant therefore runs on a thread of its own.
async fn settle_all(preparations: Vec<BoxFuture<'_, Result<Prepared>>>) -> Vec<Result<Prepared>> {
    let runtime = match tokio::runtime::Handle::try_current() {
        Ok(runtime)
            if preparations.len() > 1
                && runtime.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread =>
        {
            runtime
        }
        _ => return join_all(preparations).await,
    };
    tokio::task::block_in_place(|| settle_on_threads(&runtime, preparations))
}

fn settle_on_threads(
    runtime: &tokio::runtime::Handle,
    preparations: Vec<BoxFuture<'_, Result<Prepared>>>,
) -> Vec<Result<Prepared>> {
    std::thread::scope(|scope| {
        // Every participant starts before the first join.
        let mut participants = Vec::with_capacity(preparations.len());
        for preparation in preparations {
            participants.push(
                std::thread::Builder::new()
                    .name("pnpm-install-participant".to_string())
                    .spawn_scoped(scope, move || runtime.block_on(preparation))
                    .expect("spawn an install participant thread"),
            );
        }
        participants
            .into_iter()
            .map(join_participant)
            .collect()
    })
}

fn join_participant<Output>(participant: std::thread::ScopedJoinHandle<'_, Output>) -> Output {
    participant.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

fn publish(mut prepared: Prepared) -> Result<()> {
    for index in 0..prepared.len() {
        let Err(error) = prepared[index].publish() else {
            continue;
        };
        let rollback_errors = roll_back_through(&mut prepared, index);
        if rollback_errors.is_empty() {
            return Err(error);
        }
        for projection in prepared {
            projection.retain();
        }
        return Err(error.wrap_err(format!(
            "install publication rollback failed; retained resources for recovery: {}",
            rollback_errors.join("; "),
        )));
    }
    for projection in prepared {
        projection.retain();
    }
    Ok(())
}

/// Undo every projection up to and including the one that failed, newest
/// first, and report what could not be undone.
fn roll_back_through(prepared: &mut Prepared, index: usize) -> Vec<String> {
    let mut rollback_errors = Vec::new();
    for projection in prepared[..=index].iter_mut().rev() {
        if let Err(error) = projection.rollback() {
            rollback_errors.push(error.to_string());
        }
    }
    rollback_errors
}

#[cfg(test)]
mod tests;
